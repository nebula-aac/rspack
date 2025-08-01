use std::ops::Add;

use rspack_core::{BuildMetaExportsType, ExportsArgument, ModuleArgument, ModuleType, SpanExt};
use swc_core::{
  common::{BytePos, Span, Spanned},
  ecma::ast::{Ident, ModuleItem, Program, UnaryExpr},
};

use super::JavascriptParserPlugin;
use crate::{
  dependency::ESMCompatibilityDependency,
  utils::eval::BasicEvaluatedExpression,
  visitors::{JavascriptParser, create_traceable_error},
};

impl JavascriptParser<'_> {
  fn throw_top_level_await_error(&mut self, msg: String, span: Span) {
    self.errors.push(Box::new(create_traceable_error(
      "JavaScript parse error".into(),
      msg,
      self.source_file,
      span.into(),
    )));
  }

  fn handle_top_level_await(&mut self, allow_top_level: bool, span: Span) {
    if !allow_top_level {
      self.throw_top_level_await_error("The top-level-await experiment is not enabled (set experiments.topLevelAwait: true to enabled it)".into(), span);
    } else if self.is_esm {
      self.build_meta.has_top_level_await = true;
    } else {
      self.throw_top_level_await_error(
        "Top-level-await is only supported in EcmaScript Modules".into(),
        span,
      );
    }
  }
}

pub struct ESMDetectionParserPlugin {
  top_level_await: bool,
}

impl ESMDetectionParserPlugin {
  pub fn new(top_level_await: bool) -> Self {
    Self { top_level_await }
  }
}

// nonHarmonyIdentifiers
fn is_non_esm_identifier(name: &str) -> bool {
  name == "exports" || name == "define"
}

// Port from https://github.com/webpack/webpack/blob/main/lib/dependencies/HarmonyDetectionParserPlugin.js
impl JavascriptParserPlugin for ESMDetectionParserPlugin {
  fn program(&self, parser: &mut JavascriptParser, ast: &Program) -> Option<bool> {
    let is_strict_esm = matches!(parser.module_type, ModuleType::JsEsm);
    let is_esm = is_strict_esm
      || matches!(ast, Program::Module(module) if module.body.iter().any(|s| matches!(s, ModuleItem::ModuleDecl(_))));

    if is_esm {
      parser
        .presentational_dependencies
        .push(Box::new(ESMCompatibilityDependency));
      parser.build_meta.esm = true;
      parser.build_meta.exports_type = BuildMetaExportsType::Namespace;
      parser.build_info.strict = true;
      parser.build_info.exports_argument = ExportsArgument::WebpackExports;
    }

    if is_strict_esm {
      parser.build_meta.strict_esm_module = true;
      parser.build_info.module_argument = ModuleArgument::WebpackModule;
    }

    None
  }

  fn top_level_await_expr(
    &self,
    parser: &mut JavascriptParser,
    expr: &swc_core::ecma::ast::AwaitExpr,
  ) {
    let lo = expr.span_lo();
    let hi = lo.add(BytePos(AWAIT_LEN));
    let span = Span::new(lo, hi);
    parser.handle_top_level_await(self.top_level_await, span);
  }

  fn top_level_for_of_await_stmt(
    &self,
    parser: &mut JavascriptParser,
    stmt: &swc_core::ecma::ast::ForOfStmt,
  ) {
    let offset = 4; // "for ".len();
    let lo = stmt.span_lo().add(BytePos(offset));
    let hi = lo.add(BytePos(AWAIT_LEN));
    let span = Span::new(lo, hi);
    parser.handle_top_level_await(self.top_level_await, span);
  }

  fn evaluate_typeof<'a>(
    &self,
    parser: &mut JavascriptParser,
    expr: &'a UnaryExpr,
    for_name: &str,
  ) -> Option<BasicEvaluatedExpression<'a>> {
    (parser.is_esm && is_non_esm_identifier(for_name))
      .then(|| BasicEvaluatedExpression::with_range(expr.span().real_lo(), expr.span_hi().0))
  }

  fn r#typeof(
    &self,
    parser: &mut JavascriptParser,
    _expr: &UnaryExpr,
    for_name: &str,
  ) -> Option<bool> {
    (parser.is_esm && is_non_esm_identifier(for_name)).then_some(true)
  }

  fn identifier(
    &self,
    parser: &mut JavascriptParser,
    _ident: &Ident,
    for_name: &str,
  ) -> Option<bool> {
    (parser.is_esm && is_non_esm_identifier(for_name)).then_some(true)
  }

  fn call(
    &self,
    parser: &mut JavascriptParser,
    _expr: &swc_core::ecma::ast::CallExpr,
    for_name: &str,
  ) -> Option<bool> {
    (parser.is_esm && is_non_esm_identifier(for_name)).then_some(true)
  }
}

/// "await".len();
const AWAIT_LEN: u32 = 5;
