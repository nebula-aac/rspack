use rspack_collections::Identifier;
use rspack_core::{Compilation, RuntimeModule, impl_runtime_module};

#[impl_runtime_module]
#[derive(Debug)]
pub struct MakeNamespaceObjectRuntimeModule {
  id: Identifier,
}

impl Default for MakeNamespaceObjectRuntimeModule {
  fn default() -> Self {
    Self::with_default(Identifier::from("webpack/runtime/make_namespace_object"))
  }
}

#[async_trait::async_trait]
impl RuntimeModule for MakeNamespaceObjectRuntimeModule {
  fn name(&self) -> Identifier {
    self.id
  }

  fn template(&self) -> Vec<(String, String)> {
    vec![(
      self.id.to_string(),
      include_str!("runtime/make_namespace_object.ejs").to_string(),
    )]
  }

  async fn generate(&self, compilation: &Compilation) -> rspack_error::Result<String> {
    let source = compilation.runtime_template.render(&self.id, None)?;

    Ok(source)
  }
}
