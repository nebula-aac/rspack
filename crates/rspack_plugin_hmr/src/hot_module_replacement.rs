use cow_utils::CowUtils;
use rspack_collections::Identifier;
use rspack_core::{Compilation, RuntimeModule, impl_runtime_module};
use rspack_util::test::{HOT_TEST_DEFINE_GLOBAL, HOT_TEST_STATUS_CHANGE};

#[impl_runtime_module]
#[derive(Debug)]
pub struct HotModuleReplacementRuntimeModule {
  id: Identifier,
}

impl Default for HotModuleReplacementRuntimeModule {
  fn default() -> Self {
    Self::with_default(Identifier::from("webpack/runtime/hot_module_replacement"))
  }
}

#[async_trait::async_trait]
impl RuntimeModule for HotModuleReplacementRuntimeModule {
  fn name(&self) -> Identifier {
    self.id
  }

  async fn generate(&self, _compilation: &Compilation) -> rspack_error::Result<String> {
    Ok(
      include_str!("runtime/hot_module_replacement.js")
        .cow_replace("$HOT_TEST_GLOBAL$", &HOT_TEST_DEFINE_GLOBAL)
        .cow_replace("$HOT_TEST_STATUS$", &HOT_TEST_STATUS_CHANGE)
        .into_owned(),
    )
  }
}
