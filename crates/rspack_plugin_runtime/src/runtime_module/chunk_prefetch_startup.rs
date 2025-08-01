use itertools::Itertools;
use rspack_collections::Identifier;
use rspack_core::{
  ChunkUkey, Compilation, RuntimeGlobals, RuntimeModule, RuntimeModuleStage, impl_runtime_module,
};

#[impl_runtime_module]
#[derive(Debug)]
pub struct ChunkPrefetchStartupRuntimeModule {
  id: Identifier,
  startup_chunks: Vec<(Vec<ChunkUkey>, Vec<ChunkUkey>)>,
  chunk: Option<ChunkUkey>,
}

impl ChunkPrefetchStartupRuntimeModule {
  pub fn new(startup_chunks: Vec<(Vec<ChunkUkey>, Vec<ChunkUkey>)>) -> Self {
    Self::with_default(
      Identifier::from("webpack/runtime/chunk_prefetch_startup"),
      startup_chunks,
      None,
    )
  }
}

#[async_trait::async_trait]
impl RuntimeModule for ChunkPrefetchStartupRuntimeModule {
  fn name(&self) -> Identifier {
    self.id
  }

  fn attach(&mut self, chunk: ChunkUkey) {
    self.chunk = Some(chunk);
  }

  fn template(&self) -> Vec<(String, String)> {
    vec![(
      self.id.to_string(),
      include_str!("runtime/chunk_prefetch_startup.ejs").to_string(),
    )]
  }

  async fn generate(&self, compilation: &Compilation) -> rspack_error::Result<String> {
    let chunk_ukey = self.chunk.expect("chunk do not attached");

    let source = self
          .startup_chunks
          .iter()
          .map(|(group_chunks, child_chunks)| {
            let group_chunk_ids = group_chunks
              .iter()
              .filter_map(|c| {
                if c.to_owned().eq(&chunk_ukey) {
                  compilation
                    .chunk_by_ukey
                    .expect_get(c)
                    .id(&compilation.chunk_ids_artifact)
                } else {
                  None
                }
              })
              .collect_vec();

            let child_chunk_ids = child_chunks
              .iter()
              .filter_map(|c| {
                compilation
                  .chunk_by_ukey
                  .expect_get(c)
                  .id(&compilation.chunk_ids_artifact)
              })
              .collect_vec();

            let body = match child_chunks.len() {
              x if x < 3 => child_chunk_ids
                .iter()
                .map(|id| {
                  format!(
                    "{}({});",
                    RuntimeGlobals::PREFETCH_CHUNK,
                    serde_json::to_string(&id).expect("invalid json tostring")
                  )
                })
                .join("\n"),
              _ => {
                format!(
                  "{}.map({})",
                  serde_json::to_string(&child_chunk_ids).expect("invalid json tostring"),
                  RuntimeGlobals::PREFETCH_CHUNK
                )
              }
            };

            let source = compilation.runtime_template.render(&self.id, Some(serde_json::json!({
              "GROUP_CHUNK_IDS": serde_json::to_string(&group_chunk_ids).expect("invalid json tostring"),
              "BODY": body,
            })))?;

            Ok(source)
          }).collect::<rspack_error::Result<Vec<String>>>()?.join("\n");

    Ok(source)
  }

  fn stage(&self) -> RuntimeModuleStage {
    RuntimeModuleStage::Trigger
  }
}
