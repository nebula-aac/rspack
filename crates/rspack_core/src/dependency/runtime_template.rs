use rspack_util::json_stringify;
use rustc_hash::FxHashSet as HashSet;
use serde_json::json;
use swc_core::ecma::atoms::Atom;

use crate::{
  AsyncDependenciesBlockIdentifier, ChunkGraph, Compilation, CompilerOptions, DependenciesBlock,
  DependencyId, Environment, ExportsArgument, ExportsInfoGetter, ExportsType,
  FakeNamespaceObjectMode, GetUsedNameParam, InitFragmentExt, InitFragmentKey, InitFragmentStage,
  Module, ModuleGraph, ModuleGraphCacheArtifact, ModuleId, ModuleIdentifier, NormalInitFragment,
  PathInfo, PrefetchExportsInfoMode, RuntimeCondition, RuntimeGlobals, RuntimeSpec,
  TemplateContext, UsedName, compile_boolean_matcher_from_lists, contextify, property_access,
  to_comment, to_normal_comment,
};

pub fn runtime_condition_expression(
  chunk_graph: &ChunkGraph,
  runtime_condition: Option<&RuntimeCondition>,
  runtime: Option<&RuntimeSpec>,
  runtime_requirements: &mut RuntimeGlobals,
) -> String {
  let Some(runtime_condition) = runtime_condition else {
    return "true".to_string();
  };

  let runtime_condition = match runtime_condition {
    RuntimeCondition::Boolean(v) => return v.to_string(),
    RuntimeCondition::Spec(spec) => spec,
  };

  let mut positive_runtime_ids = HashSet::default();
  for_each_runtime(
    Some(runtime_condition),
    |runtime| {
      if let Some(runtime_id) =
        runtime.and_then(|runtime| chunk_graph.get_runtime_id(runtime.clone()))
      {
        positive_runtime_ids.insert(runtime_id);
      }
    },
    false,
  );

  let mut negative_runtime_ids = HashSet::default();
  for_each_runtime(
    subtract_runtime(runtime, Some(runtime_condition)).as_ref(),
    |runtime| {
      if let Some(runtime_id) =
        runtime.and_then(|runtime| chunk_graph.get_runtime_id(runtime.clone()))
      {
        negative_runtime_ids.insert(runtime_id);
      }
    },
    false,
  );

  runtime_requirements.insert(RuntimeGlobals::RUNTIME_ID);

  compile_boolean_matcher_from_lists(
    positive_runtime_ids.into_iter().collect::<Vec<_>>(),
    negative_runtime_ids.into_iter().collect::<Vec<_>>(),
  )
  .render(RuntimeGlobals::RUNTIME_ID.to_string().as_str())
}

fn subtract_runtime(a: Option<&RuntimeSpec>, b: Option<&RuntimeSpec>) -> Option<RuntimeSpec> {
  match (a, b) {
    (Some(a), None) => Some(a.clone()),
    (None, None) => None,
    (None, Some(b)) => Some(b.clone()),
    (Some(a), Some(b)) => Some(a.subtract(b)),
  }
}

pub fn for_each_runtime<F>(runtime: Option<&RuntimeSpec>, mut f: F, deterministic_order: bool)
where
  F: FnMut(Option<&String>),
{
  match runtime {
    None => f(None),
    Some(runtime) => {
      if deterministic_order {
        let mut runtimes = runtime.iter().collect::<Vec<_>>();
        runtimes.sort();
        for r in runtimes {
          f(Some(&r.to_string()));
        }
      } else {
        for r in runtime.iter() {
          f(Some(&r.to_string()));
        }
      }
    }
  }
}

#[allow(clippy::too_many_arguments)]
pub fn export_from_import(
  code_generatable_context: &mut TemplateContext,
  default_interop: bool,
  request: &str,
  import_var: &str,
  export_name: &[Atom],
  id: &DependencyId,
  is_call: bool,
  call_context: bool,
  asi_safe: Option<bool>,
) -> String {
  let TemplateContext {
    runtime_requirements,
    compilation,
    init_fragments,
    module,
    runtime,
    ..
  } = code_generatable_context;
  let Some(module_identifier) = compilation
    .get_module_graph()
    .module_identifier_by_dependency_id(id)
    .copied()
  else {
    return missing_module(request);
  };

  let exports_type = get_exports_type(
    &compilation.get_module_graph(),
    &compilation.module_graph_cache_artifact,
    id,
    &module.identifier(),
  );

  let mut exclude_default_export_name = None;
  if default_interop {
    if !export_name.is_empty()
      && let Some(first_export_name) = export_name.first()
      && first_export_name == "default"
    {
      match exports_type {
        ExportsType::Dynamic => {
          if is_call {
            return format!("{import_var}_default(){}", property_access(export_name, 1));
          } else {
            return if let Some(asi_safe) = asi_safe {
              match asi_safe {
                true => format!(
                  "({import_var}_default(){})",
                  property_access(export_name, 1)
                ),
                false => format!(
                  ";({import_var}_default(){})",
                  property_access(export_name, 1)
                ),
              }
            } else {
              format!("{import_var}_default.a{}", property_access(export_name, 1))
            };
          }
        }
        ExportsType::DefaultOnly | ExportsType::DefaultWithNamed => {
          exclude_default_export_name = Some(export_name[1..].to_vec());
        }
        _ => {}
      }
    } else if !export_name.is_empty() {
      if matches!(exports_type, ExportsType::DefaultOnly) {
        return format!(
          "/* non-default import from non-esm module */undefined\n{}",
          property_access(export_name, 1)
        );
      } else if !matches!(exports_type, ExportsType::Namespace)
        && let Some(first_export_name) = export_name.first()
        && first_export_name == "__esModule"
      {
        return "/* __esModule */true".to_string();
      }
    } else if matches!(
      exports_type,
      ExportsType::DefaultOnly | ExportsType::DefaultWithNamed
    ) {
      runtime_requirements.insert(RuntimeGlobals::CREATE_FAKE_NAMESPACE_OBJECT);

      let name = format!("var {import_var}_namespace_cache;\n");
      init_fragments.push(
        NormalInitFragment::new(
          name.clone(),
          InitFragmentStage::StageESMExports,
          -1,
          InitFragmentKey::ESMFakeNamespaceObjectFragment(name),
          None,
        )
        .boxed(),
      );
      let prefix = if let Some(asi_safe) = asi_safe {
        match asi_safe {
          true => "",
          false => ";",
        }
      } else {
        "Object"
      };
      return format!(
        "/*#__PURE__*/ {prefix}({import_var}_namespace_cache || ({import_var}_namespace_cache = {}({import_var}{})))",
        RuntimeGlobals::CREATE_FAKE_NAMESPACE_OBJECT,
        if matches!(exports_type, ExportsType::DefaultOnly) {
          ""
        } else {
          ", 2"
        }
      );
    }
  }

  let export_name = exclude_default_export_name
    .as_deref()
    .unwrap_or(export_name);
  if !export_name.is_empty() {
    let used_name = match ExportsInfoGetter::get_used_name(
      GetUsedNameParam::WithNames(&compilation.get_module_graph().get_prefetched_exports_info(
        &module_identifier,
        PrefetchExportsInfoMode::Nested(export_name),
      )),
      *runtime,
      export_name,
    ) {
      Some(UsedName::Normal(used_name)) => used_name,
      Some(UsedName::Inlined(inlined)) => {
        return format!(
          "{} {}",
          to_normal_comment(&format!(
            "inlined export {}",
            property_access(export_name, 0)
          )),
          inlined.render()
        );
      }
      None => {
        return format!(
          "{} undefined",
          to_normal_comment(&format!(
            "unused export {}",
            property_access(export_name, 0)
          ))
        );
      }
    };
    let comment = if used_name != export_name {
      to_normal_comment(&property_access(export_name, 0))
    } else {
      String::new()
    };
    let property = property_access(used_name, 0);
    let access = format!("{import_var}{comment}{property}");
    if is_call && !call_context {
      if let Some(asi_safe) = asi_safe {
        match asi_safe {
          true => format!("(0,{access})"),
          false => format!(";(0,{access})"),
        }
      } else {
        format!("Object({access})")
      }
    } else {
      format!("{import_var}{comment}{property}")
    }
  } else {
    import_var.to_string()
  }
}

pub fn get_exports_type(
  module_graph: &ModuleGraph,
  module_graph_cache: &ModuleGraphCacheArtifact,
  id: &DependencyId,
  parent_module: &ModuleIdentifier,
) -> ExportsType {
  let strict = module_graph
    .module_by_identifier(parent_module)
    .expect("should have mgm")
    .get_strict_esm_module();
  get_exports_type_with_strict(module_graph, module_graph_cache, id, strict)
}

pub fn get_exports_type_with_strict(
  module_graph: &ModuleGraph,
  module_graph_cache: &ModuleGraphCacheArtifact,
  id: &DependencyId,
  strict: bool,
) -> ExportsType {
  let module = module_graph
    .module_identifier_by_dependency_id(id)
    .expect("should have module");
  module_graph
    .module_by_identifier(module)
    .expect("should have module")
    .get_exports_type(module_graph, module_graph_cache, strict)
}

// information content of the comment
#[derive(Default)]
struct CommentOptions<'a> {
  // request string used originally
  request: Option<&'a str>,
  // name of the chunk referenced
  chunk_name: Option<&'a str>,
  // additional message
  message: Option<&'a str>,
}

// add a comment
fn comment(compiler_options: &CompilerOptions, comment_options: CommentOptions) -> String {
  let used_pathinfo = matches!(
    compiler_options.output.pathinfo,
    PathInfo::Bool(true) | PathInfo::String(_)
  );
  let content = if used_pathinfo {
    vec![
      comment_options.message,
      comment_options.request,
      comment_options.chunk_name,
    ]
  } else {
    vec![comment_options.message, comment_options.chunk_name]
  }
  .iter()
  .filter_map(|&item| item)
  .map(|item| contextify(compiler_options.context.as_path(), item))
  .collect::<Vec<_>>()
  .join(" | ");

  if content.is_empty() {
    return String::new();
  }

  if used_pathinfo {
    format!("{} ", to_comment(&content))
  } else {
    format!("{} ", to_normal_comment(&content))
  }
}

pub fn module_id_expr(
  compiler_options: &CompilerOptions,
  request: &str,
  module_id: &ModuleId,
) -> String {
  format!(
    "{}{}",
    comment(
      compiler_options,
      CommentOptions {
        request: Some(request),
        ..Default::default()
      }
    ),
    json_stringify(module_id)
  )
}

pub fn module_id(
  compilation: &Compilation,
  id: &DependencyId,
  request: &str,
  weak: bool,
) -> String {
  if let Some(module_identifier) = compilation
    .get_module_graph()
    .module_identifier_by_dependency_id(id)
    && let Some(module_id) =
      ChunkGraph::get_module_id(&compilation.module_ids_artifact, *module_identifier)
  {
    module_id_expr(&compilation.options, request, module_id)
  } else if weak {
    "null /* weak dependency, without id */".to_string()
  } else {
    missing_module(request)
  }
}

pub fn import_statement(
  module: &dyn Module,
  compilation: &Compilation,
  runtime_requirements: &mut RuntimeGlobals,
  id: &DependencyId,
  request: &str,
  update: bool, // whether a new variable should be created or the existing one updated
) -> (String, String) {
  if compilation
    .get_module_graph()
    .module_identifier_by_dependency_id(id)
    .is_none()
  {
    return (missing_module_statement(request), String::new());
  };

  let module_id_expr = module_id(compilation, id, request, false);

  runtime_requirements.insert(RuntimeGlobals::REQUIRE);

  let import_var = compilation.get_import_var(id);

  let opt_declaration = if update { "" } else { "var " };

  let import_content = format!(
    "/* ESM import */{opt_declaration}{import_var} = {}({module_id_expr});\n",
    RuntimeGlobals::REQUIRE
  );

  let exports_type = get_exports_type(
    &compilation.get_module_graph(),
    &compilation.module_graph_cache_artifact,
    id,
    &module.identifier(),
  );
  if matches!(exports_type, ExportsType::Dynamic) {
    runtime_requirements.insert(RuntimeGlobals::COMPAT_GET_DEFAULT_EXPORT);
    return (
      import_content,
      format!(
        "/* ESM import */{opt_declaration}{import_var}_default = /*#__PURE__*/{}({import_var});\n",
        RuntimeGlobals::COMPAT_GET_DEFAULT_EXPORT,
      ),
    );
  }
  (import_content, String::new())
}

pub fn module_namespace_promise(
  code_generatable_context: &mut TemplateContext,
  dep_id: &DependencyId,
  block: Option<&AsyncDependenciesBlockIdentifier>,
  request: &str,
  message: &str,
  weak: bool,
) -> String {
  let TemplateContext {
    runtime_requirements,
    compilation,
    module,
    ..
  } = code_generatable_context;
  if compilation
    .get_module_graph()
    .module_identifier_by_dependency_id(dep_id)
    .is_none()
  {
    return missing_module_promise(request);
  };

  let promise = block_promise(block, runtime_requirements, compilation, message);
  let exports_type = get_exports_type(
    &compilation.get_module_graph(),
    &compilation.module_graph_cache_artifact,
    dep_id,
    &module.identifier(),
  );
  let module_id_expr = module_id(compilation, dep_id, request, weak);

  let header = if weak {
    runtime_requirements.insert(RuntimeGlobals::MODULE_FACTORIES);
    Some(format!(
      "if(!{}[{module_id_expr}]) {{\n {} \n}}",
      RuntimeGlobals::MODULE_FACTORIES,
      weak_error(request)
    ))
  } else {
    None
  };
  let mut fake_type = FakeNamespaceObjectMode::PROMISE_LIKE;
  let mut appending;
  match exports_type {
    ExportsType::Namespace => {
      if let Some(header) = header {
        appending = format!(
          ".then(function() {{ {header}\nreturn {}}})",
          module_raw(compilation, runtime_requirements, dep_id, request, weak)
        )
      } else {
        runtime_requirements.insert(RuntimeGlobals::REQUIRE);
        appending = format!(
          ".then({}.bind({}, {module_id_expr}))",
          RuntimeGlobals::REQUIRE,
          RuntimeGlobals::REQUIRE
        );
      }
    }
    _ => {
      if matches!(exports_type, ExportsType::Dynamic) {
        fake_type |= FakeNamespaceObjectMode::RETURN_VALUE;
      }
      if matches!(
        exports_type,
        ExportsType::DefaultWithNamed | ExportsType::Dynamic
      ) {
        fake_type |= FakeNamespaceObjectMode::MERGE_PROPERTIES;
      }
      runtime_requirements.insert(RuntimeGlobals::CREATE_FAKE_NAMESPACE_OBJECT);
      if ModuleGraph::is_async(
        compilation,
        compilation
          .get_module_graph()
          .module_identifier_by_dependency_id(dep_id)
          .expect("should have module"),
      ) {
        if let Some(header) = header {
          appending = format!(
            ".then(function() {{\n {header}\nreturn {}\n}})",
            module_raw(compilation, runtime_requirements, dep_id, request, weak)
          )
        } else {
          runtime_requirements.insert(RuntimeGlobals::REQUIRE);
          appending = format!(
            ".then({}.bind({}, {module_id_expr}))",
            RuntimeGlobals::REQUIRE,
            RuntimeGlobals::REQUIRE
          );
        }
        appending.push_str(
          format!(
            ".then(function(m){{\n return {}(m, {fake_type}) \n}})",
            RuntimeGlobals::CREATE_FAKE_NAMESPACE_OBJECT
          )
          .as_str(),
        );
      } else {
        fake_type |= FakeNamespaceObjectMode::MODULE_ID;
        if let Some(header) = header {
          let expr = format!(
            "{}({module_id_expr}, {fake_type}))",
            RuntimeGlobals::CREATE_FAKE_NAMESPACE_OBJECT
          );
          appending = format!(".then(function() {{\n {header} return {expr};\n}})");
        } else {
          runtime_requirements.insert(RuntimeGlobals::REQUIRE);
          appending = format!(
            ".then({}.bind({}, {module_id_expr}, {fake_type}))",
            RuntimeGlobals::CREATE_FAKE_NAMESPACE_OBJECT,
            RuntimeGlobals::REQUIRE
          );
        }
      }
    }
  }

  format!("{promise}{appending}")
}

pub fn block_promise(
  block: Option<&AsyncDependenciesBlockIdentifier>,
  runtime_requirements: &mut RuntimeGlobals,
  compilation: &Compilation,
  message: &str,
) -> String {
  let Some(block) = block else {
    let comment = comment(
      &compilation.options,
      CommentOptions {
        request: None,
        chunk_name: None,
        message: Some(message),
      },
    );
    return format!("Promise.resolve({})", comment.trim());
  };
  let chunk_group = compilation
    .chunk_graph
    .get_block_chunk_group(block, &compilation.chunk_group_by_ukey);
  let Some(chunk_group) = chunk_group else {
    let comment = comment(
      &compilation.options,
      CommentOptions {
        request: None,
        chunk_name: None,
        message: Some(message),
      },
    );
    return format!("Promise.resolve({})", comment.trim());
  };
  if chunk_group.chunks.is_empty() {
    let comment = comment(
      &compilation.options,
      CommentOptions {
        request: None,
        chunk_name: None,
        message: Some(message),
      },
    );
    return format!("Promise.resolve({})", comment.trim());
  }
  let mg = compilation.get_module_graph();
  let block = mg.block_by_id_expect(block);
  let comment = comment(
    &compilation.options,
    CommentOptions {
      request: None,
      chunk_name: block.get_group_options().and_then(|o| o.name()),
      message: Some(message),
    },
  );
  let chunks = chunk_group
    .chunks
    .iter()
    .map(|c| compilation.chunk_by_ukey.expect_get(c))
    .filter(|c| {
      !c.has_runtime(&compilation.chunk_group_by_ukey)
        && c.id(&compilation.chunk_ids_artifact).is_some()
    })
    .collect::<Vec<_>>();

  if chunks.len() == 1 {
    let chunk_id = serde_json::to_string(
      chunks[0]
        .id(&compilation.chunk_ids_artifact)
        .expect("should have chunk.id"),
    )
    .expect("should able to json stringify");
    runtime_requirements.insert(RuntimeGlobals::ENSURE_CHUNK);

    let fetch_priority = chunk_group
      .kind
      .get_normal_options()
      .and_then(|x| x.fetch_priority);

    if fetch_priority.is_some() {
      runtime_requirements.insert(RuntimeGlobals::HAS_FETCH_PRIORITY);
    }

    format!(
      "{}({comment}{chunk_id}{})",
      RuntimeGlobals::ENSURE_CHUNK,
      fetch_priority
        .map(|x| format!(r#", "{x}""#))
        .unwrap_or_default()
    )
  } else if !chunks.is_empty() {
    runtime_requirements.insert(RuntimeGlobals::ENSURE_CHUNK);

    let fetch_priority = chunk_group
      .kind
      .get_normal_options()
      .and_then(|x| x.fetch_priority);

    if fetch_priority.is_some() {
      runtime_requirements.insert(RuntimeGlobals::HAS_FETCH_PRIORITY);
    }

    format!(
      "Promise.all({comment}[{}])",
      chunks
        .iter()
        .map(|c| format!(
          "{}({}{})",
          RuntimeGlobals::ENSURE_CHUNK,
          serde_json::to_string(
            c.id(&compilation.chunk_ids_artifact)
              .expect("should have chunk.id")
          )
          .expect("should able to json stringify"),
          fetch_priority
            .map(|x| format!(r#", "{x}""#))
            .unwrap_or_default()
        ))
        .collect::<Vec<_>>()
        .join(", ")
    )
  } else {
    format!("Promise.resolve({})", comment.trim())
  }
}

pub fn module_raw(
  compilation: &Compilation,
  runtime_requirements: &mut RuntimeGlobals,
  id: &DependencyId,
  request: &str,
  weak: bool,
) -> String {
  if let Some(module_identifier) = compilation
    .get_module_graph()
    .module_identifier_by_dependency_id(id)
    && let Some(module_id) =
      ChunkGraph::get_module_id(&compilation.module_ids_artifact, *module_identifier)
  {
    runtime_requirements.insert(RuntimeGlobals::REQUIRE);
    format!(
      "{}({})",
      RuntimeGlobals::REQUIRE,
      module_id_expr(&compilation.options, request, module_id)
    )
  } else if weak {
    weak_error(request)
  } else {
    missing_module(request)
  }
}

fn missing_module(request: &str) -> String {
  format!("Object({}())", throw_missing_module_error_function(request))
}

fn missing_module_statement(request: &str) -> String {
  format!("{};\n", missing_module(request))
}

pub fn missing_module_promise(request: &str) -> String {
  format!(
    "Promise.resolve().then({})",
    throw_missing_module_error_function(request)
  )
}

fn throw_missing_module_error_function(request: &str) -> String {
  format!(
    "function webpackMissingModule() {{ {} }}",
    throw_missing_module_error_block(request)
  )
}

pub fn throw_missing_module_error_block(request: &str) -> String {
  let e = format!("Cannot find module '{request}'");
  format!(
    "var e = new Error({}); e.code = 'MODULE_NOT_FOUND'; throw e;",
    json!(e)
  )
}

pub fn weak_error(request: &str) -> String {
  format!(
    "var e = new Error('Module is not available (weak dependency), request is {request}'); e.code = 'MODULE_NOT_FOUND'; throw e;"
  )
}

pub fn returning_function(environment: &Environment, return_value: &str, args: &str) -> String {
  if environment.supports_arrow_function() {
    format!("({args}) => ({return_value})")
  } else {
    format!("function({args}) {{ return {return_value}; }}")
  }
}

pub fn basic_function(environment: &Environment, args: &str, body: &str) -> String {
  if environment.supports_arrow_function() {
    format!("({args}) => {{\n{body}\n}}")
  } else {
    format!("function({args}) {{\n{body}\n}}")
  }
}

pub fn sync_module_factory(
  dep: &DependencyId,
  request: &str,
  compilation: &Compilation,
  runtime_requirements: &mut RuntimeGlobals,
) -> String {
  let factory = returning_function(
    &compilation.options.output.environment,
    &module_raw(compilation, runtime_requirements, dep, request, false),
    "",
  );
  returning_function(&compilation.options.output.environment, &factory, "")
}

pub fn async_module_factory(
  block_id: &AsyncDependenciesBlockIdentifier,
  request: &str,
  compilation: &Compilation,
  runtime_requirements: &mut RuntimeGlobals,
) -> String {
  let module_graph = compilation.get_module_graph();
  let block = module_graph
    .block_by_id(block_id)
    .expect("should have block");
  let dep = block.get_dependencies()[0];
  let ensure_chunk = block_promise(Some(block_id), runtime_requirements, compilation, "");
  let factory = returning_function(
    &compilation.options.output.environment,
    &module_raw(compilation, runtime_requirements, &dep, request, false),
    "",
  );
  returning_function(
    &compilation.options.output.environment,
    &if ensure_chunk.starts_with("Promise.resolve(") {
      factory
    } else {
      format!(
        "{ensure_chunk}.then({})",
        returning_function(&compilation.options.output.environment, &factory, "")
      )
    },
    "",
  )
}

pub fn define_es_module_flag_statement(
  exports_argument: ExportsArgument,
  runtime_requirements: &mut RuntimeGlobals,
) -> String {
  runtime_requirements.insert(RuntimeGlobals::MAKE_NAMESPACE_OBJECT);
  runtime_requirements.insert(RuntimeGlobals::EXPORTS);

  format!(
    "{}({});\n",
    RuntimeGlobals::MAKE_NAMESPACE_OBJECT,
    exports_argument
  )
}
#[allow(unused_imports)]
mod test_items_to_regexp {
  use crate::items_to_regexp;

  #[test]
  fn basic() {
    assert_eq!(
      items_to_regexp(
        vec!["a", "b", "c", "d", "ef"]
          .into_iter()
          .map(String::from)
          .collect::<Vec<_>>(),
      ),
      "([abcd]|ef)".to_string()
    );

    assert_eq!(
      items_to_regexp(
        vec!["a1", "a2", "a3", "a4", "b5"]
          .into_iter()
          .map(String::from)
          .collect::<Vec<_>>(),
      ),
      "(a[1234]|b5)".to_string()
    );

    assert_eq!(
      items_to_regexp(
        vec!["a1", "b1", "c1", "d1", "e2"]
          .into_iter()
          .map(String::from)
          .collect::<Vec<_>>(),
      ),
      "([abcd]1|e2)".to_string()
    );

    assert_eq!(
      items_to_regexp(
        vec!["1", "2", "3", "4", "a"]
          .into_iter()
          .map(String::from)
          .collect::<Vec<_>>(),
      ),
      "[1234a]".to_string()
    );

    assert_eq!(
      items_to_regexp(
        vec!["foo_js", "_js"]
          .into_iter()
          .map(String::from)
          .collect::<Vec<_>>(),
      ),
      "(|foo)_js".to_string()
    );
  }

  #[test]
  fn multibyte() {
    assert_eq!(
      items_to_regexp(
        vec!["🍉", "🍊", "🍓", "🍐", "🍍🫙"]
          .into_iter()
          .map(String::from)
          .collect::<Vec<_>>(),
      ),
      "([🍉🍊🍐🍓]|🍍🫙)".to_string()
    );

    assert_eq!(
      items_to_regexp(
        vec!["🫙🍉", "🫙🍊", "🫙🍓", "🫙🍐", "🍽🍍"]
          .into_iter()
          .map(String::from)
          .collect::<Vec<_>>(),
      ),
      "(🫙[🍉🍊🍐🍓]|🍽🍍)".to_string()
    );

    assert_eq!(
      items_to_regexp(
        vec!["🍉🍭", "🍊🍭", "🍓🍭", "🍐🍭", "🍍🫙"]
          .into_iter()
          .map(String::from)
          .collect::<Vec<_>>(),
      ),
      "([🍉🍊🍐🍓]🍭|🍍🫙)".to_string()
    );

    assert_eq!(
      items_to_regexp(
        vec!["🍉", "🍊", "🍓", "🍐", "🫙"]
          .into_iter()
          .map(String::from)
          .collect::<Vec<_>>(),
      ),
      "[🍉🍊🍐🍓🫙]".to_string()
    );
  }
}
