use std::{hash::Hash, sync::LazyLock};

use itertools::Itertools;
use regex::Regex;
use rspack_core::{
  AsyncDependenciesBlock, ConstDependency, DependencyRange, EntryOptions, GroupOptions,
  SharedSourceMap, SpanExt,
};
use rspack_hash::RspackHash;
use rustc_hash::{FxHashMap, FxHashSet};
use swc_core::{
  atoms::Atom,
  common::{Span, Spanned},
  ecma::ast::{CallExpr, Expr, ExprOrSpread, Ident, NewExpr, VarDecl, VarDeclarator},
};

use super::{
  JavascriptParserPlugin,
  esm_import_dependency_parser_plugin::{ESM_SPECIFIER_TAG, ESMSpecifierData},
  url_plugin::get_url_request,
};
use crate::{
  dependency::{CreateScriptUrlDependency, WorkerDependency},
  utils::object_properties::get_literal_str_by_obj_prop,
  visitors::{JavascriptParser, TagInfoData},
  webpack_comment::try_extract_webpack_magic_comment,
};

#[derive(Debug)]
struct ParsedNewWorkerPath {
  pub range: (u32, u32),
  pub value: String,
}

#[derive(Debug)]
struct ParsedNewWorkerOptions {
  pub range: Option<(u32, u32)>,
  pub name: Option<String>,
}

fn parse_new_worker_options(arg: &ExprOrSpread) -> ParsedNewWorkerOptions {
  let obj = arg.expr.as_object();
  let name = obj
    .and_then(|obj| get_literal_str_by_obj_prop(obj, "name"))
    .map(|str| str.value.to_string());
  let span = arg.span();
  ParsedNewWorkerOptions {
    range: Some((span.real_lo(), span.real_hi())),
    name,
  }
}

fn parse_new_worker_options_from_comments(
  parser: &mut JavascriptParser,
  span: Span,
  diagnostic_span: Span,
) -> Option<ParsedNewWorkerOptions> {
  let comments = try_extract_webpack_magic_comment(
    parser.source_file,
    &parser.comments,
    diagnostic_span,
    span,
    &mut parser.warning_diagnostics,
  );
  comments
    .get_webpack_chunk_name()
    .map(|name| ParsedNewWorkerOptions {
      range: None,
      name: Some(name.to_string()),
    })
}

fn add_dependencies(
  parser: &mut JavascriptParser,
  span: Span,
  first_arg: &ExprOrSpread,
  parsed_path: ParsedNewWorkerPath,
  parsed_options: Option<ParsedNewWorkerOptions>,
) {
  let output_options = &parser.compiler_options.output;
  let mut hasher = RspackHash::from(output_options);
  parser.module_identifier.hash(&mut hasher);
  parser.worker_index.hash(&mut hasher);
  parser.worker_index += 1;
  let digest = hasher.digest(&output_options.hash_digest);
  let runtime = digest
    .rendered(output_options.hash_digest_length)
    .to_owned();
  let range = parsed_options.as_ref().and_then(|options| options.range);
  let name = parsed_options.and_then(|options| options.name);
  let output_module = output_options.module;
  let dep = Box::new(WorkerDependency::new(
    parsed_path.value,
    output_options.worker_public_path.clone(),
    span.into(),
    parsed_path.range.into(),
  ));
  let source_map: SharedSourceMap = parser.source_map.clone();
  let mut block = AsyncDependenciesBlock::new(
    *parser.module_identifier,
    Into::<DependencyRange>::into(span).to_loc(Some(&source_map)),
    None,
    vec![dep],
    None,
  );
  block.set_group_options(GroupOptions::Entrypoint(Box::new(EntryOptions {
    name,
    runtime: Some(runtime.into()),
    chunk_loading: Some(output_options.worker_chunk_loading.clone()),
    async_chunks: None,
    public_path: None,
    base_uri: None,
    filename: None,
    library: None,
    depend_on: None,
    layer: None,
  })));

  parser.blocks.push(Box::new(block));

  if parser.compiler_options.output.trusted_types.is_some() {
    parser
      .dependencies
      .push(Box::new(CreateScriptUrlDependency::new(
        span.into(),
        first_arg.span().into(),
      )));
  }

  if let Some(range) = range {
    parser
      .presentational_dependencies
      .push(Box::new(ConstDependency::new(
        (range.0, range.0).into(),
        "Object.assign({}, ".into(),
        None,
      )));
    parser
      .presentational_dependencies
      .push(Box::new(ConstDependency::new(
        (range.1, range.1).into(),
        format!(
          ", {{ type: {} }})",
          if output_module {
            "\"module\""
          } else {
            "undefined"
          }
        )
        .into(),
        None,
      )));
  }
}

fn handle_worker<'a>(
  parser: &mut JavascriptParser,
  args: &'a [ExprOrSpread],
  span: Span,
) -> Option<(
  ParsedNewWorkerPath,
  Option<ParsedNewWorkerOptions>,
  &'a ExprOrSpread,
)> {
  if let Some(expr_or_spread) = args.first()
    && let ExprOrSpread {
      spread: None,
      expr: expr_box,
    } = expr_or_spread
    && let Expr::New(new_url_expr) = &**expr_box
    && let Some((request, start, end)) = get_url_request(parser, new_url_expr)
  {
    let path = ParsedNewWorkerPath {
      range: (start, end),
      value: request,
    };
    let options = args
      .get(1)
      // new Worker(new URL("worker.js"), options)
      .map(parse_new_worker_options)
      .or_else(|| {
        // new Worker(new URL(/* options */ "worker.js"))
        new_url_expr
          .args
          .as_ref()
          .and_then(|args| args.first())
          .and_then(|n| {
            parse_new_worker_options_from_comments(parser, n.span(), new_url_expr.span())
          })
      })
      .or_else(|| {
        // new Worker(/* options */ new URL("worker.js"))
        parse_new_worker_options_from_comments(parser, expr_or_spread.span(), span)
      });
    Some((path, options, expr_or_spread))
  } else {
    None
  }
}

pub struct WorkerPlugin {
  new_syntax: FxHashSet<String>,
  call_syntax: FxHashSet<String>,
  from_new_syntax: FxHashSet<(String, String)>,
  from_call_syntax: FxHashSet<(String, String)>,
  pattern_syntax: FxHashMap<String, FxHashSet<String>>,
}

static WORKER_FROM_REGEX: LazyLock<Regex> =
  LazyLock::new(|| Regex::new(r"^(.+?)(\(\))?\s+from\s+(.+)$").expect("invalid regex"));

const WORKER_SPECIFIER_TAG: &str = "_identifier__worker_specifier_tag__";
const DEFAULT_SYNTAX: [&str; 4] = [
  "Worker",
  "SharedWorker",
  "navigator.serviceWorker.register()",
  "Worker from worker_threads",
];

#[derive(Debug, Clone)]
struct WorkerSpecifierData {
  key: Atom,
}

impl WorkerPlugin {
  pub fn new(syntax_list: &[String]) -> Self {
    let mut this = Self {
      new_syntax: FxHashSet::default(),
      call_syntax: FxHashSet::default(),
      from_new_syntax: FxHashSet::default(),
      from_call_syntax: FxHashSet::default(),
      pattern_syntax: FxHashMap::default(),
    };
    for syntax in syntax_list {
      if syntax == "..." {
        for syntax in DEFAULT_SYNTAX {
          this.handle_syntax(syntax);
        }
      } else {
        this.handle_syntax(syntax);
      }
    }
    this
  }

  fn handle_syntax(&mut self, syntax: &str) {
    if let Some(syntax) = syntax.strip_prefix('*')
      && let Some(first_dot) = syntax.find('.')
      && let Some(syntax) = syntax.strip_suffix("()")
    {
      let pattern = &syntax[0..first_dot];
      let members = &syntax[first_dot + 1..];
      if let Some(value) = self.pattern_syntax.get_mut(pattern) {
        value.insert(members.to_string());
      } else {
        self.pattern_syntax.insert(
          pattern.to_string(),
          FxHashSet::from_iter([members.to_string()]),
        );
      }
    } else if let Some(syntax) = syntax.strip_suffix("()") {
      self.call_syntax.insert(syntax.to_string());
    } else if let Some(captures) = WORKER_FROM_REGEX.captures(syntax) {
      let ids = &captures[1];
      let is_call = &captures.get(2).is_some();
      let source = &captures[3];
      if *is_call {
        self
          .from_call_syntax
          .insert((ids.to_string(), source.to_string()));
      } else {
        self
          .from_new_syntax
          .insert((ids.to_string(), source.to_string()));
      }
    } else {
      self.new_syntax.insert(syntax.to_string());
    }
  }
}

impl JavascriptParserPlugin for WorkerPlugin {
  fn pre_declarator(
    &self,
    parser: &mut JavascriptParser,
    decl: &VarDeclarator,
    _statement: &VarDecl,
  ) -> Option<bool> {
    if let Some(ident) = decl.name.as_ident()
      && self.pattern_syntax.contains_key(ident.sym.as_str())
    {
      parser.tag_variable(
        ident.sym.to_string(),
        WORKER_SPECIFIER_TAG,
        Some(WorkerSpecifierData {
          key: ident.sym.clone(),
        }),
      );
      return Some(true);
    }
    None
  }

  fn pattern(&self, parser: &mut JavascriptParser, ident: &Ident, for_name: &str) -> Option<bool> {
    if self.pattern_syntax.contains_key(for_name) {
      parser.tag_variable(
        ident.sym.to_string(),
        WORKER_SPECIFIER_TAG,
        Some(WorkerSpecifierData {
          key: ident.sym.clone(),
        }),
      );
      return Some(true);
    }
    None
  }

  fn call_member_chain(
    &self,
    parser: &mut JavascriptParser,
    call_expr: &CallExpr,
    for_name: &str,
    members: &[Atom],
    _members_optionals: &[bool],
    _member_ranges: &[Span],
  ) -> Option<bool> {
    if for_name != WORKER_SPECIFIER_TAG {
      return None;
    }
    let tag_info = parser
      .definitions_db
      .expect_get_tag_info(parser.current_tag_info?);
    let data = WorkerSpecifierData::downcast(tag_info.data.clone()?);
    if let Some(value) = self.pattern_syntax.get(data.key.as_str())
      && value.contains(&members.iter().map(|id| id.as_str()).join("."))
    {
      return handle_worker(parser, &call_expr.args, call_expr.span).map(
        |(parsed_path, parsed_options, first_arg)| {
          add_dependencies(
            parser,
            call_expr.span,
            first_arg,
            parsed_path,
            parsed_options,
          );
          if let Some(callee) = call_expr.callee.as_expr() {
            parser.walk_expression(callee);
          }
          if let Some(arg) = call_expr.args.get(1) {
            parser.walk_expression(&arg.expr);
          }
          true
        },
      );
    }
    None
  }

  fn call(
    &self,
    parser: &mut JavascriptParser,
    call_expr: &CallExpr,
    for_name: &str,
  ) -> Option<bool> {
    if for_name == ESM_SPECIFIER_TAG {
      let tag_info = parser
        .definitions_db
        .expect_get_tag_info(parser.current_tag_info?);
      let settings = ESMSpecifierData::downcast(tag_info.data.clone()?);
      let ids = settings.ids.iter().map(|id| id.as_str()).join(".");
      if self
        .from_call_syntax
        .contains(&(ids, settings.source.to_string()))
      {
        return handle_worker(parser, &call_expr.args, call_expr.span).map(
          |(parsed_path, parsed_options, first_arg)| {
            add_dependencies(
              parser,
              call_expr.span,
              first_arg,
              parsed_path,
              parsed_options,
            );
            if let Some(callee) = call_expr.callee.as_expr() {
              parser.walk_expression(callee);
            }
            if let Some(arg) = call_expr.args.get(1) {
              parser.walk_expression(&arg.expr);
            }
            true
          },
        );
      }
      return None;
    }
    if !self.call_syntax.contains(for_name) {
      return None;
    }
    handle_worker(parser, &call_expr.args, call_expr.span).map(
      |(parsed_path, parsed_options, first_arg)| {
        add_dependencies(
          parser,
          call_expr.span,
          first_arg,
          parsed_path,
          parsed_options,
        );
        if let Some(callee) = call_expr.callee.as_expr() {
          parser.walk_expression(callee);
        }
        if let Some(arg) = call_expr.args.get(1) {
          parser.walk_expression(&arg.expr);
        }
        true
      },
    )
  }

  fn new_expression(
    &self,
    parser: &mut JavascriptParser,
    new_expr: &NewExpr,
    for_name: &str,
  ) -> Option<bool> {
    if for_name == ESM_SPECIFIER_TAG {
      let tag_info = parser
        .definitions_db
        .expect_get_tag_info(parser.current_tag_info?);
      let settings = ESMSpecifierData::downcast(tag_info.data.clone()?);
      let ids = settings.ids.iter().map(|id| id.as_str()).join(".");
      if self
        .from_new_syntax
        .contains(&(ids, settings.source.to_string()))
      {
        return new_expr
          .args
          .as_ref()
          .and_then(|args| handle_worker(parser, args, new_expr.span))
          .map(|(parsed_path, parsed_options, first_arg)| {
            add_dependencies(
              parser,
              new_expr.span,
              first_arg,
              parsed_path,
              parsed_options,
            );
            parser.walk_expression(&new_expr.callee);
            if let Some(args) = &new_expr.args
              && let Some(arg) = args.get(1)
            {
              parser.walk_expression(&arg.expr);
            }
            true
          });
      }
      return None;
    }
    if !self.new_syntax.contains(for_name) {
      return None;
    }
    new_expr
      .args
      .as_ref()
      .and_then(|args| handle_worker(parser, args, new_expr.span))
      .map(|(parsed_path, parsed_options, first_arg)| {
        add_dependencies(
          parser,
          new_expr.span,
          first_arg,
          parsed_path,
          parsed_options,
        );
        parser.walk_expression(&new_expr.callee);
        if let Some(args) = &new_expr.args
          && let Some(arg) = args.get(1)
        {
          parser.walk_expression(&arg.expr);
        }
        true
      })
  }
}
