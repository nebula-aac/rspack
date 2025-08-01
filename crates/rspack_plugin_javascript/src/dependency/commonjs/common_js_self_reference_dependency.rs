use rspack_cacheable::{
  cacheable, cacheable_dyn,
  with::{AsPreset, AsVec},
};
use rspack_core::{
  AsContextDependency, Dependency, DependencyCategory, DependencyCodeGeneration, DependencyId,
  DependencyRange, DependencyTemplate, DependencyTemplateType, DependencyType, ExportsInfoGetter,
  ExtendedReferencedExport, FactorizeInfo, GetUsedNameParam, ModuleDependency, ModuleGraph,
  ModuleGraphCacheArtifact, PrefetchExportsInfoMode, RuntimeGlobals, RuntimeSpec, TemplateContext,
  TemplateReplaceSource, UsedName, property_access,
};
use swc_core::atoms::Atom;

use super::ExportsBase;

#[cacheable]
#[derive(Debug, Clone)]
pub struct CommonJsSelfReferenceDependency {
  id: DependencyId,
  range: DependencyRange,
  base: ExportsBase,
  #[cacheable(with=AsVec<AsPreset>)]
  names: Vec<Atom>,
  is_call: bool,
  factorize_info: FactorizeInfo,
}

impl CommonJsSelfReferenceDependency {
  pub fn new(range: DependencyRange, base: ExportsBase, names: Vec<Atom>, is_call: bool) -> Self {
    Self {
      id: DependencyId::new(),
      range,
      base,
      names,
      is_call,
      factorize_info: Default::default(),
    }
  }
}

#[cacheable_dyn]
impl Dependency for CommonJsSelfReferenceDependency {
  fn id(&self) -> &DependencyId {
    &self.id
  }

  fn range(&self) -> Option<&DependencyRange> {
    Some(&self.range)
  }

  fn category(&self) -> &DependencyCategory {
    &DependencyCategory::CommonJS
  }

  fn dependency_type(&self) -> &DependencyType {
    &DependencyType::CjsSelfReference
  }

  fn resource_identifier(&self) -> Option<&str> {
    Some("self")
  }

  fn get_referenced_exports(
    &self,
    _module_graph: &ModuleGraph,
    _module_graph_cache: &ModuleGraphCacheArtifact,
    _runtime: Option<&RuntimeSpec>,
  ) -> Vec<ExtendedReferencedExport> {
    if self.is_call {
      if self.names.is_empty() {
        vec![ExtendedReferencedExport::Array(vec![])]
      } else {
        vec![ExtendedReferencedExport::Array(
          self.names[0..self.names.len() - 1].to_vec(),
        )]
      }
    } else {
      vec![ExtendedReferencedExport::Array(self.names.clone())]
    }
  }

  fn could_affect_referencing_module(&self) -> rspack_core::AffectType {
    rspack_core::AffectType::True
  }
}

#[cacheable_dyn]
impl ModuleDependency for CommonJsSelfReferenceDependency {
  fn request(&self) -> &str {
    "self"
  }

  fn factorize_info(&self) -> &FactorizeInfo {
    &self.factorize_info
  }

  fn factorize_info_mut(&mut self) -> &mut FactorizeInfo {
    &mut self.factorize_info
  }
}

impl AsContextDependency for CommonJsSelfReferenceDependency {}

#[cacheable_dyn]
impl DependencyCodeGeneration for CommonJsSelfReferenceDependency {
  fn dependency_template(&self) -> Option<DependencyTemplateType> {
    Some(CommonJsSelfReferenceDependencyTemplate::template_type())
  }
}

#[cacheable]
#[derive(Debug, Clone, Default)]
pub struct CommonJsSelfReferenceDependencyTemplate;

impl CommonJsSelfReferenceDependencyTemplate {
  pub fn template_type() -> DependencyTemplateType {
    DependencyTemplateType::Dependency(DependencyType::CjsSelfReference)
  }
}

impl DependencyTemplate for CommonJsSelfReferenceDependencyTemplate {
  fn render(
    &self,
    dep: &dyn DependencyCodeGeneration,
    source: &mut TemplateReplaceSource,
    code_generatable_context: &mut TemplateContext,
  ) {
    let dep = dep
      .as_any()
      .downcast_ref::<CommonJsSelfReferenceDependency>()
      .expect("CommonJsSelfReferenceDependencyTemplate should only be used for CommonJsSelfReferenceDependency");

    let TemplateContext {
      compilation,
      module,
      runtime,
      runtime_requirements,
      ..
    } = code_generatable_context;
    let module_graph = compilation.get_module_graph();
    let module = module_graph
      .module_by_identifier(&module.identifier())
      .expect("should have mgm");

    let used = if dep.names.is_empty() {
      let used_name = if dep.names.is_empty() {
        let exports_info = ExportsInfoGetter::prefetch_used_info_without_name(
          &module_graph.get_exports_info(&module.identifier()),
          &module_graph,
          *runtime,
          false,
        );
        ExportsInfoGetter::get_used_name(
          GetUsedNameParam::WithoutNames(&exports_info),
          *runtime,
          &dep.names,
        )
      } else {
        let exports_info = module_graph.get_prefetched_exports_info(
          &module.identifier(),
          PrefetchExportsInfoMode::Nested(&dep.names),
        );
        ExportsInfoGetter::get_used_name(
          GetUsedNameParam::WithNames(&exports_info),
          *runtime,
          &dep.names,
        )
      };

      used_name.unwrap_or_else(|| UsedName::Normal(dep.names.clone()))
    } else {
      UsedName::Normal(dep.names.clone())
    };

    let exports_argument = module.get_exports_argument();
    let module_argument = module.get_module_argument();

    let base = if dep.base.is_exports() {
      runtime_requirements.insert(RuntimeGlobals::EXPORTS);
      exports_argument.to_string()
    } else if dep.base.is_module_exports() {
      runtime_requirements.insert(RuntimeGlobals::MODULE);
      format!("{module_argument}.exports")
    } else if dep.base.is_this() {
      runtime_requirements.insert(RuntimeGlobals::THIS_AS_EXPORTS);
      "this".to_string()
    } else {
      unreachable!();
    };

    source.replace(
      dep.range.start,
      dep.range.end,
      &match used {
        UsedName::Normal(used) => format!("{}{}", base, property_access(used, 0)),
        // Export a inlinable const from cjs is not possible for now, so self reference a inlinable
        // const is also not possible for now, but we compat it here
        UsedName::Inlined(inlined) => inlined.render().into_owned(),
      },
      None,
    )
  }
}
