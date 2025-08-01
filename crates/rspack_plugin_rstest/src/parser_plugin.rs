use camino::Utf8PathBuf;
use rspack_core::{
  AsyncDependenciesBlock, ConstDependency, Dependency, DependencyRange, ImportAttributes,
  SharedSourceMap, SpanExt,
};
use rspack_plugin_javascript::{
  JavascriptParserPlugin,
  dependency::{CommonJsRequireDependency, ImportDependency, RequireHeaderDependency},
  utils::{
    self,
    eval::{self},
  },
  visitors::JavascriptParser,
};
use rspack_util::{atom::Atom, json_stringify, swc::get_swc_comments};
use swc_core::{
  common::{Span, Spanned},
  ecma::ast::{CallExpr, Ident, MemberExpr, UnaryExpr},
};

static RSTEST_MOCK_FIRST_ARG_TAG: &str = "strip the import call from the first arg of mock series";

use crate::{
  mock_method_dependency::{MockMethod, MockMethodDependency, Position},
  mock_module_id_dependency::MockModuleIdDependency,
  module_path_name_dependency::{ModulePathNameDependency, NameType},
};

const DIR_NAME: &str = "__dirname";
const FILE_NAME: &str = "__filename";
const IMPORT_META_DIRNAME: &str = "import.meta.dirname";
const IMPORT_META_FILENAME: &str = "import.meta.filename";

#[derive(PartialEq)]
enum ModulePathType {
  DirName,
  FileName,
}

#[derive(Debug, Default)]
pub struct RstestParserPlugin {
  pub module_path_name: bool,
  pub hoist_mock_module: bool,
  pub import_meta_path_name: bool,
  pub manual_mock_root: String,
}

trait JavascriptParserExt<'a> {
  fn handle_top_level_await(&mut self);
}

impl<'a> JavascriptParserExt<'a> for JavascriptParser<'a> {
  fn handle_top_level_await(&mut self) {
    self.build_meta.has_top_level_await = true;
  }
}

impl RstestParserPlugin {
  pub fn new(
    module_path_name: bool,
    hoist_mock_module: bool,
    import_meta_path_name: bool,
    manual_mock_root: String,
  ) -> Self {
    Self {
      module_path_name,
      hoist_mock_module,
      import_meta_path_name,
      manual_mock_root,
    }
  }

  fn process_require_actual(
    &self,
    parser: &mut JavascriptParser,
    call_expr: &CallExpr,
  ) -> Option<bool> {
    match call_expr.args.len() {
      1 => {
        let first_arg = &call_expr.args[0];
        if let Some(lit) = first_arg.expr.as_lit()
          && let Some(lit) = lit.as_str()
        {
          let mut range_expr: DependencyRange = first_arg.span().into();
          range_expr.end += 1; // TODO:
          let dep = CommonJsRequireDependency::new(
            lit.value.to_string(),
            range_expr,
            Some(call_expr.span.into()),
            parser.in_try,
            Some(parser.source_map.clone()),
          );
          parser.dependencies.push(Box::new(dep));

          let range: DependencyRange = call_expr.callee.span().into();
          parser
            .presentational_dependencies
            .push(Box::new(RequireHeaderDependency::new(
              range.clone(),
              Some(parser.source_map.clone()),
            )));

          parser
            .presentational_dependencies
            .push(Box::new(ConstDependency::new(
              range,
              ".rstest_require_actual".into(),
              None,
            )));

          return Some(true);
        }
      }
      _ => {
        panic!("`rs.importActual` function expects 1 argument");
      }
    }

    None
  }

  fn process_import_actual(
    &self,
    parser: &mut JavascriptParser,
    call_expr: &CallExpr,
  ) -> Option<bool> {
    match call_expr.args.len() {
      1 => {
        let first_arg = &call_expr.args[0];
        if let Some(lit) = first_arg.expr.as_lit()
          && let Some(lit) = lit.as_str()
        {
          let mut attrs = ImportAttributes::default();
          attrs.insert("rstest".to_string(), "importActual".to_string());

          let imported_span = call_expr.args.first().expect("should have one arg");

          let dep = Box::new(ImportDependency::new(
            Atom::from(lit.value.as_ref()),
            call_expr.span.into(),
            None,
            Some(attrs),
            parser.in_try,
            get_swc_comments(
              parser.comments,
              imported_span.span().lo,
              imported_span.span().hi,
            ),
          ));

          let source_map: SharedSourceMap = parser.source_map.clone();
          let block = AsyncDependenciesBlock::new(
            *parser.module_identifier,
            Into::<DependencyRange>::into(call_expr.span).to_loc(Some(&source_map)),
            None,
            vec![dep],
            Some(lit.value.to_string()),
          );

          parser.blocks.push(Box::new(block));
          return Some(true);
        }
      }
      _ => {
        panic!("`rs.importActual` function expects 1 argument");
      }
    }

    None
  }

  fn calc_mocked_target(&self, value: &str) -> Utf8PathBuf {
    // node:foo will be mocked to `__mocks__/foo`.
    let path_buf = Utf8PathBuf::from(
      value
        .to_string()
        .strip_prefix("node:")
        .unwrap_or(value.as_ref())
        .to_string(),
    );
    let is_relative_request = path_buf.to_string().starts_with("."); // TODO: consider alias?

    if is_relative_request {
      // Mock relative request to alongside `__mocks__` directory.
      path_buf
        .parent()
        .map(|p| {
          p.join("__mocks__")
            .join(path_buf.file_name().unwrap_or_default())
        })
        .unwrap_or_else(|| Utf8PathBuf::from("__mocks__").join(path_buf))
    } else {
      // Mock non-relative request to `manual_mock_root` directory.
      Utf8PathBuf::from(&self.manual_mock_root).join(&path_buf)
    }
  }

  fn handle_mock_first_arg(
    &self,
    parser: &mut JavascriptParser,
    mock_call_expr: &CallExpr,
  ) -> Option<String> {
    let first_arg = &mock_call_expr.args[0];
    let mut is_import_call = false;

    if let Some(first_arg) = mock_call_expr.args.first()
      && let Some(import_call) = first_arg.expr.as_call()
      && import_call.callee.as_import().is_some()
    {
      parser.tag_variable::<bool>(
        self.compose_rstest_import_call_key(import_call),
        RSTEST_MOCK_FIRST_ARG_TAG,
        Some(true),
      );
      is_import_call = true;
    }

    let lit_str = if is_import_call {
      first_arg
        .expr
        .as_call()
        .and_then(|expr| expr.args.first())
        .and_then(|arg| arg.expr.as_lit())
        .and_then(|lit| lit.as_str())
        .map(|lit| lit.value.as_str())
    } else {
      first_arg
        .expr
        .as_lit()
        .and_then(|lit| lit.as_str())
        .map(|lit| lit.value.as_str())
    };

    lit_str.map(|s| s.to_string())
  }

  #[allow(clippy::too_many_arguments)]
  fn process_mock(
    &self,
    parser: &mut JavascriptParser,
    call_expr: &CallExpr,
    hoist: bool,
    is_esm: bool,
    method: MockMethod,
    has_b: bool,
    async_factory: bool,
  ) {
    match call_expr.args.len() {
      1 => {
        let first_arg = &call_expr.args[0];
        let first_arg_lit_str = self.handle_mock_first_arg(parser, call_expr);

        if let Some(lit_str) = first_arg_lit_str {
          if hoist && method != MockMethod::Unmock && method != MockMethod::DoMock {
            parser.handle_top_level_await();
          }

          if let Some(mocked_target) = self.calc_mocked_target(&lit_str).as_std_path().to_str() {
            let dep = MockModuleIdDependency::new(
              lit_str.to_string(),
              first_arg.span().into(),
              false,
              true,
              if is_esm {
                rspack_core::DependencyCategory::Esm
              } else {
                rspack_core::DependencyCategory::CommonJS
              },
              if has_b { Some(", ".to_string()) } else { None },
              hoist,
              async_factory,
            );
            let id = *dep.id();
            parser.dependencies.push(Box::new(dep));

            parser
              .presentational_dependencies
              .push(Box::new(MockMethodDependency::new(
                call_expr.span(),
                call_expr.callee.span(),
                lit_str,
                hoist,
                method,
                Some(id),
                Position::Before,
              )));

            if has_b {
              let second_arg = Span::new(
                first_arg.span().hi() + swc_core::common::BytePos(0),
                first_arg.span().hi() + swc_core::common::BytePos(0),
              );
              parser
                .dependencies
                .push(Box::new(MockModuleIdDependency::new(
                  mocked_target.to_string(),
                  second_arg.into(),
                  false,
                  true,
                  if is_esm {
                    rspack_core::DependencyCategory::Esm
                  } else {
                    rspack_core::DependencyCategory::CommonJS
                  },
                  None,
                  hoist,
                  false,
                )));
            }
          }
        }
      }
      // mock a module
      2 => {
        let first_arg = &call_expr.args[0];
        let second_arg = &call_expr.args[1];

        if first_arg.spread.is_some() || second_arg.spread.is_some() {
          return;
        }

        let lit_str = self.handle_mock_first_arg(parser, call_expr);

        if let Some(lit_str) = lit_str {
          if hoist {
            parser.handle_top_level_await();
          }

          let module_dep = MockModuleIdDependency::new(
            lit_str.to_string(),
            first_arg.span().into(),
            false,
            true,
            if is_esm {
              rspack_core::DependencyCategory::Esm
            } else {
              rspack_core::DependencyCategory::CommonJS
            },
            None,
            hoist,
            true,
          );

          parser
            .presentational_dependencies
            .push(Box::new(MockMethodDependency::new(
              call_expr.span(),
              call_expr.callee.span(),
              lit_str,
              hoist,
              method,
              Some(*module_dep.id()),
              Position::After,
            )));
          parser.dependencies.push(Box::new(module_dep));
        } else {
          panic!("`rs.mock` function expects a string literal as the first argument");
        }
      }
      _ => {
        panic!("`rs.mock` function expects 1 or 2 arguments, got more than 2");
      }
    }
  }

  fn hoisted(&self, parser: &mut JavascriptParser, call_expr: &CallExpr) {
    match call_expr.args.len() {
      1 => {
        parser
          .presentational_dependencies
          .push(Box::new(MockMethodDependency::new(
            call_expr.span(),
            call_expr.callee.span(),
            call_expr.span().real_lo().to_string(),
            true,
            MockMethod::Hoisted,
            None,
            Position::Before,
          )));
      }
      _ => {
        panic!("`rs.hoisted` function expects 1 argument, got more than 1");
      }
    }
  }

  fn reset_modules(&self, parser: &mut JavascriptParser, call_expr: &CallExpr) -> Option<bool> {
    match call_expr.args.len() {
      0 => {
        parser
          .presentational_dependencies
          .push(Box::new(ConstDependency::new(
            call_expr.callee.span().into(),
            "__webpack_require__.rstest_reset_modules".into(),
            None,
          )));
        Some(true)
      }
      _ => {
        panic!("`rs.resetModules` function expects 0 arguments, got more than 0");
      }
    }
  }

  fn load_mock(
    &self,
    parser: &mut JavascriptParser,
    call_expr: &CallExpr,
    is_esm: bool,
  ) -> Option<bool> {
    match call_expr.args.len() {
      1 => {
        let first_arg = &call_expr.args[0];
        if let Some(lit) = first_arg.expr.as_lit() {
          if let Some(lit) = lit.as_str() {
            if let Some(mocked_target) = self.calc_mocked_target(&lit.value).as_std_path().to_str()
            {
              if is_esm {
                let imported_span = call_expr.args.first().expect("should have one arg");

                let mut attrs = ImportAttributes::default();
                attrs.insert("rstest".to_string(), "importMock".to_string());
                let dep = Box::new(ImportDependency::new(
                  Atom::from(mocked_target),
                  call_expr.span.into(),
                  None,
                  Some(attrs),
                  parser.in_try,
                  get_swc_comments(
                    parser.comments,
                    imported_span.span().lo,
                    imported_span.span().hi,
                  ),
                ));

                let source_map: SharedSourceMap = parser.source_map.clone();
                let block = AsyncDependenciesBlock::new(
                  *parser.module_identifier,
                  Into::<DependencyRange>::into(call_expr.span).to_loc(Some(&source_map)),
                  None,
                  vec![dep],
                  Some(mocked_target.to_string()),
                );

                parser.blocks.push(Box::new(block));

                return Some(true);
              } else {
                // add CommonJsRequireDependency
                let mut range_expr: DependencyRange = first_arg.span().into();
                range_expr.end += 1; // TODO:
                let dep: CommonJsRequireDependency = CommonJsRequireDependency::new(
                  mocked_target.to_string(),
                  range_expr,
                  Some(call_expr.span.into()),
                  parser.in_try,
                  Some(parser.source_map.clone()),
                );

                let range: DependencyRange = call_expr.callee.span().into();
                parser
                  .presentational_dependencies
                  .push(Box::new(RequireHeaderDependency::new(
                    range,
                    Some(parser.source_map.clone()),
                  )));

                parser.dependencies.push(Box::new(dep));
                return Some(true);
              }
            }
          } else {
            return None;
          }
        }

        None
      }
      _ => {
        panic!("`load_mock` function expects 1 argument, got more than 1");
      }
    }
  }

  fn process_import_meta(&self, parser: &mut JavascriptParser, r#type: ModulePathType) -> String {
    if r#type == ModulePathType::FileName {
      if let Some(resource_path) = &parser.resource_data.resource_path {
        json_stringify(&resource_path.clone().into_string())
      } else {
        "''".to_string()
      }
    } else {
      let resource_path = parser
        .resource_data
        .resource_path
        .as_deref()
        .and_then(|p| p.parent())
        .map(|p| p.to_string())
        .unwrap_or_default();
      json_stringify(&resource_path)
    }
  }

  fn compose_rstest_import_call_key(&self, call_expr: &CallExpr) -> String {
    format!(
      "rstest_strip_import_call {} {}",
      call_expr.span.real_lo(),
      call_expr.span.real_hi(),
    )
  }
}

impl JavascriptParserPlugin for RstestParserPlugin {
  fn import_call(&self, parser: &mut JavascriptParser, call_expr: &CallExpr) -> Option<bool> {
    let first_arg = self.handle_mock_first_arg(parser, call_expr);
    if first_arg.is_some() {
      let tag_data = parser.get_tag_data(
        &Atom::from(self.compose_rstest_import_call_key(call_expr).as_str()),
        RSTEST_MOCK_FIRST_ARG_TAG,
      );

      if tag_data.is_some() {
        return Some(true);
      }
    }

    None
  }

  fn call_member_chain(
    &self,
    parser: &mut JavascriptParser,
    call_expr: &CallExpr,
    _for_name: &str,
    _members: &[Atom],
    _members_optionals: &[bool],
    _member_ranges: &[Span],
  ) -> Option<bool> {
    if self.hoist_mock_module {
      let expr = call_expr.callee.as_expr();
      if let Some(expr) = expr {
        let q = expr.as_member();
        if let Some(q) = q {
          if let Some(ident) = q.obj.as_ident()
            && let Some(prop) = q.prop.as_ident()
          {
            match (ident.sym.as_str(), prop.sym.as_str()) {
              // rs.mock
              ("rs", "mock") | ("rstest", "mock") => {
                self.process_mock(parser, call_expr, true, true, MockMethod::Mock, true, true);
                return Some(false);
              }
              // rs.mockRequire
              ("rs", "mockRequire") | ("rstest", "mockRequire") => {
                self.process_mock(
                  parser,
                  call_expr,
                  true,
                  false,
                  MockMethod::Mock,
                  true,
                  false,
                );
                return Some(false);
              }
              // rs.doMock
              ("rs", "doMock") | ("rstest", "doMock") => {
                self.process_mock(
                  parser,
                  call_expr,
                  false,
                  true,
                  MockMethod::DoMock,
                  true,
                  false,
                );
                return Some(false);
              }
              // rs.doMockRequire
              ("rs", "doMockRequire") | ("rstest", "doMockRequire") => {
                self.process_mock(
                  parser,
                  call_expr,
                  false,
                  false,
                  MockMethod::Mock,
                  true,
                  false,
                );
                return Some(false);
              }
              // rs.importActual
              ("rs", "importActual") | ("rstest", "importActual") => {
                return self.process_import_actual(parser, call_expr);
              }
              // rs.requireActual
              ("rs", "requireActual") | ("rstest", "requireActual") => {
                return self.process_require_actual(parser, call_expr);
              }
              // rs.importMock
              ("rs", "importMock") | ("rstest", "importMock") => {
                return self.load_mock(parser, call_expr, true);
              }
              // rs.requireMock
              ("rs", "requireMock") | ("rstest", "requireMock") => {
                return self.load_mock(parser, call_expr, false);
              }
              // rs.unmock
              ("rs", "unmock") | ("rstest", "unmock") => {
                self.process_mock(
                  parser,
                  call_expr,
                  true,
                  true,
                  MockMethod::Unmock,
                  false,
                  false,
                );
                return Some(true);
              }
              // rs.doUnmock
              ("rs", "doUnmock") | ("rstest", "doUnmock") => {
                // return self.unmock_method(parser, call_expr, true);
                self.process_mock(
                  parser,
                  call_expr,
                  false,
                  true,
                  MockMethod::Unmock,
                  false,
                  false,
                );
                return Some(true);
              }
              // rs.resetModules
              ("rs", "resetModules") | ("rstest", "resetModules") => {
                return self.reset_modules(parser, call_expr);
              }
              // rs.hoisted
              ("rs", "hoisted") | ("rstest", "hoisted") => {
                self.hoisted(parser, call_expr);
                return Some(true);
              }
              _ => {
                // Not a mock module, continue.
                return None;
              }
            }
          }
        } else {
          return None;
        }
      }
    }
    None
  }

  fn identifier(
    &self,
    parser: &mut rspack_plugin_javascript::visitors::JavascriptParser,
    ident: &Ident,
    _for_name: &str,
  ) -> Option<bool> {
    let str = ident.sym.as_str();
    if !parser.is_unresolved_ident(str) {
      return None;
    }

    if self.module_path_name {
      match str {
        DIR_NAME => {
          parser
            .presentational_dependencies
            .push(Box::new(ModulePathNameDependency::new(NameType::DirName)));
          return Some(true);
        }
        FILE_NAME => {
          parser
            .presentational_dependencies
            .push(Box::new(ModulePathNameDependency::new(NameType::FileName)));
          return Some(true);
        }
        _ => return None,
      }
    }

    None
  }

  fn evaluate_typeof<'a>(
    &self,
    _parser: &mut JavascriptParser,
    expr: &'a UnaryExpr,
    for_name: &str,
  ) -> Option<utils::eval::BasicEvaluatedExpression<'a>> {
    if self.import_meta_path_name {
      let mut evaluated = None;
      if for_name == IMPORT_META_DIRNAME || for_name == IMPORT_META_FILENAME {
        evaluated = Some("string".to_string());
      }
      return evaluated
        .map(|e| eval::evaluate_to_string(e, expr.span.real_lo(), expr.span.real_hi()));
    }

    None
  }

  fn evaluate_identifier(
    &self,
    parser: &mut JavascriptParser,
    ident: &str,
    start: u32,
    end: u32,
  ) -> Option<eval::BasicEvaluatedExpression<'static>> {
    if self.import_meta_path_name {
      if ident == IMPORT_META_DIRNAME {
        return Some(eval::evaluate_to_string(
          self.process_import_meta(parser, ModulePathType::DirName),
          start,
          end,
        ));
      } else if ident == IMPORT_META_FILENAME {
        return Some(eval::evaluate_to_string(
          self.process_import_meta(parser, ModulePathType::FileName),
          start,
          end,
        ));
      } else {
        return None;
      }
    }
    None
  }

  fn r#typeof(
    &self,
    parser: &mut JavascriptParser,
    unary_expr: &UnaryExpr,
    for_name: &str,
  ) -> Option<bool> {
    if self.import_meta_path_name {
      if for_name == IMPORT_META_DIRNAME || for_name == IMPORT_META_FILENAME {
        parser
          .presentational_dependencies
          .push(Box::new(ConstDependency::new(
            unary_expr.span().into(),
            "'string'".into(),
            None,
          )));
        return Some(true);
      } else {
        return None;
      }
    }

    None
  }

  fn member(
    &self,
    parser: &mut JavascriptParser,
    member_expr: &MemberExpr,
    for_name: &str,
  ) -> Option<bool> {
    if self.import_meta_path_name {
      if for_name == IMPORT_META_DIRNAME {
        let result = self.process_import_meta(parser, ModulePathType::DirName);
        parser
          .presentational_dependencies
          .push(Box::new(ConstDependency::new(
            member_expr.span().into(),
            result.into(),
            None,
          )));
        return Some(true);
      } else if for_name == IMPORT_META_FILENAME {
        let result = self.process_import_meta(parser, ModulePathType::FileName);
        parser
          .presentational_dependencies
          .push(Box::new(ConstDependency::new(
            member_expr.span().into(),
            result.into(),
            None,
          )));
        return Some(true);
      } else {
        return None;
      }
    }

    None
  }
}
