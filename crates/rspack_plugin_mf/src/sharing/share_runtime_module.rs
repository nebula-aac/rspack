use hashlink::{LinkedHashMap, LinkedHashSet};
use itertools::Itertools;
use rspack_collections::Identifier;
use rspack_core::{
  ChunkUkey, Compilation, ModuleId, RuntimeGlobals, RuntimeModule, SourceType, impl_runtime_module,
};
use rustc_hash::FxHashMap;

use super::provide_shared_plugin::ProvideVersion;
use crate::{ConsumeVersion, utils::json_stringify};

#[impl_runtime_module]
#[derive(Debug)]
pub struct ShareRuntimeModule {
  id: Identifier,
  chunk: Option<ChunkUkey>,
  enhanced: bool,
}

impl ShareRuntimeModule {
  pub fn new(enhanced: bool) -> Self {
    Self::with_default(Identifier::from("webpack/runtime/sharing"), None, enhanced)
  }
}

#[async_trait::async_trait]
impl RuntimeModule for ShareRuntimeModule {
  fn name(&self) -> Identifier {
    self.id
  }

  async fn generate(&self, compilation: &Compilation) -> rspack_error::Result<String> {
    let chunk_ukey = self
      .chunk
      .expect("should have chunk in <ShareRuntimeModule as RuntimeModule>::generate");
    let chunk = compilation.chunk_by_ukey.expect_get(&chunk_ukey);
    let module_graph = compilation.get_module_graph();
    let mut init_per_scope: FxHashMap<
      String,
      LinkedHashMap<DataInitStage, LinkedHashSet<DataInitInfo>>,
    > = FxHashMap::default();
    for c in chunk.get_all_referenced_chunks(&compilation.chunk_group_by_ukey) {
      let chunk = compilation.chunk_by_ukey.expect_get(&c);
      let modules = compilation
        .chunk_graph
        .get_chunk_modules_iterable_by_source_type(&c, SourceType::ShareInit, &module_graph)
        .sorted_unstable_by_key(|m| m.identifier());
      for m in modules {
        let code_gen = compilation
          .code_generation_results
          .get(&m.identifier(), Some(chunk.runtime()));
        let Some(data) = code_gen.data.get::<CodeGenerationDataShareInit>() else {
          continue;
        };
        for item in &data.items {
          let stages = init_per_scope.entry(item.share_scope.clone()).or_default();
          let list = stages
            .entry(item.init_stage)
            .or_insert_with(LinkedHashSet::default);
          list.insert(item.init.clone());
        }
      }
    }
    let scope_to_data_init = init_per_scope
      .into_iter()
      .sorted_unstable_by_key(|(scope, _)| scope.to_string())
      .map(|(scope, stages)| {
        let stages: Vec<String> = stages
          .into_iter()
          .sorted_unstable_by_key(|(stage, _)| *stage)
          .flat_map(|(_, inits)| inits)
          .map(|info| match info {
            DataInitInfo::ExternalModuleId(Some(id)) => json_stringify(&id),
            DataInitInfo::ProvideSharedInfo(info) => {
              let mut stage = format!(
                "{{ name: {}, version: {}, factory: {}, eager: {}",
                json_stringify(&info.name),
                json_stringify(&info.version.to_string()),
                info.factory,
                if info.eager { "1" } else { "0" },
              );
              if self.enhanced {
                if let Some(singleton) = info.singleton {
                  stage += ", singleton: ";
                  stage += if singleton { "1" } else { "0" };
                }
                if let Some(required_version) = info.required_version {
                  stage += ", requiredVersion: ";
                  stage += &json_stringify(&required_version.to_string());
                }
                if let Some(strict_version) = info.strict_version {
                  stage += ", strictVersion: ";
                  stage += if strict_version { "1" } else { "0" };
                }
              }
              stage += " }";
              stage
            }
            _ => "".to_string(),
          })
          .collect();
        format!("{}: [{}]", json_stringify(&scope), stages.join(", "))
      })
      .collect::<Vec<_>>()
      .join(", ");
    let initialize_sharing_impl = if self.enhanced {
      "__webpack_require__.I = __webpack_require__.I || function() { throw new Error(\"should have __webpack_require__.I\") }"
    } else {
      include_str!("./initializeSharing.js")
    };
    Ok(format!(
      r#"
{share_scope_map} = {{}};
__webpack_require__.initializeSharingData = {{ scopeToSharingDataMapping: {{ {scope_to_data_init} }}, uniqueName: {unique_name} }};
{initialize_sharing_impl}
"#,
      share_scope_map = RuntimeGlobals::SHARE_SCOPE_MAP,
      scope_to_data_init = scope_to_data_init,
      unique_name = json_stringify(&compilation.options.output.unique_name),
      initialize_sharing_impl = initialize_sharing_impl,
    ))
  }

  fn attach(&mut self, chunk: ChunkUkey) {
    self.chunk = Some(chunk);
  }
}

#[derive(Debug, Clone)]
pub struct CodeGenerationDataShareInit {
  pub items: Vec<ShareInitData>,
}

#[derive(Debug, Clone)]
pub struct ShareInitData {
  pub share_scope: String,
  pub init_stage: DataInitStage,
  pub init: DataInitInfo,
}

pub type DataInitStage = i8;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum DataInitInfo {
  ExternalModuleId(Option<ModuleId>),
  ProvideSharedInfo(ProvideSharedInfo),
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ProvideSharedInfo {
  pub name: String,
  pub version: ProvideVersion,
  pub factory: String,
  pub eager: bool,
  pub singleton: Option<bool>,
  pub required_version: Option<ConsumeVersion>,
  pub strict_version: Option<bool>,
}
