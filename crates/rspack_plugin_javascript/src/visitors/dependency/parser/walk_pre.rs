use std::borrow::Cow;

use rustc_hash::FxHashSet;
use swc_core::{
  common::Spanned,
  ecma::ast::{
    AssignExpr, BlockStmt, CatchClause, Decl, DoWhileStmt, ForInStmt, ForOfStmt, ForStmt, IfStmt,
    LabeledStmt, ModuleDecl, ModuleItem, ObjectPat, ObjectPatProp, Stmt, SwitchCase, SwitchStmt,
    TryStmt, VarDecl, VarDeclKind, VarDeclarator, WhileStmt, WithStmt,
  },
};

use super::{
  DestructuringAssignmentProperty, JavascriptParser,
  estree::{MaybeNamedFunctionDecl, Statement},
};
use crate::{parser_plugin::JavascriptParserPlugin, utils::eval};

impl JavascriptParser<'_> {
  pub fn pre_walk_module_items(&mut self, statements: &Vec<ModuleItem>) {
    for statement in statements {
      self.pre_walk_module_item(statement);
    }
  }

  pub fn pre_walk_statements(&mut self, statements: &Vec<Stmt>) {
    for statement in statements {
      self.pre_walk_statement(statement.into())
    }
  }

  fn pre_walk_module_item(&mut self, statement: &ModuleItem) {
    match statement {
      ModuleItem::ModuleDecl(decl) => {
        match decl {
          ModuleDecl::TsImportEquals(_)
          | ModuleDecl::TsExportAssignment(_)
          | ModuleDecl::TsNamespaceExport(_) => unreachable!(),
          _ => {
            self.is_esm = true;
          }
        };
      }
      ModuleItem::Stmt(stmt) => self.pre_walk_statement(stmt.into()),
    }
  }

  pub fn pre_walk_statement(&mut self, statement: Statement) {
    self.enter_statement(
      &statement,
      |parser, _| {
        parser
          .plugin_drive
          .clone()
          .pre_statement(parser, statement)
          .unwrap_or_default()
      },
      |parser, _| {
        match statement {
          Statement::Block(stmt) => parser.pre_walk_block_statement(stmt),
          Statement::DoWhile(stmt) => parser.pre_walk_do_while_statement(stmt),
          Statement::ForIn(stmt) => parser.pre_walk_for_in_statement(stmt),
          Statement::ForOf(stmt) => parser.pre_walk_for_of_statement(stmt),
          Statement::For(stmt) => parser.pre_walk_for_statement(stmt),
          Statement::Fn(stmt) => parser.pre_walk_function_declaration(stmt),
          Statement::Var(stmt) => parser.pre_walk_variable_declaration(stmt),
          Statement::If(stmt) => parser.pre_walk_if_statement(stmt),
          Statement::Labeled(stmt) => parser.pre_walk_labeled_statement(stmt),
          Statement::Switch(stmt) => parser.pre_walk_switch_statement(stmt),
          Statement::Try(stmt) => parser.pre_walk_try_statement(stmt),
          Statement::While(stmt) => parser.pre_walk_while_statement(stmt),
          Statement::With(stmt) => parser.pre_walk_with_statement(stmt),
          _ => (),
        };
      },
    );
  }

  pub fn pre_walk_declaration(&mut self, decl: &Decl) {
    match decl {
      Decl::Fn(decl) => self.pre_walk_function_declaration(decl.into()),
      Decl::Var(decl) => self.pre_walk_variable_declaration(decl),
      Decl::Class(_) | Decl::Using(_) => (),
      Decl::TsInterface(_) | Decl::TsTypeAlias(_) | Decl::TsEnum(_) | Decl::TsModule(_) => {
        unreachable!()
      }
    }
  }

  fn pre_walk_with_statement(&mut self, stmt: &WithStmt) {
    self.pre_walk_statement(stmt.body.as_ref().into())
  }

  fn pre_walk_while_statement(&mut self, stmt: &WhileStmt) {
    self.pre_walk_statement(stmt.body.as_ref().into())
  }

  fn pre_walk_catch_clause(&mut self, cache_clause: &CatchClause) {
    self.pre_walk_statement(Statement::Block(&cache_clause.body));
  }

  fn pre_walk_try_statement(&mut self, stmt: &TryStmt) {
    self.pre_walk_statement(Statement::Block(&stmt.block));
    if let Some(handler) = &stmt.handler {
      self.pre_walk_catch_clause(handler)
    }
    if let Some(finalizer) = &stmt.finalizer {
      self.pre_walk_statement(Statement::Block(finalizer));
    }
  }

  fn pre_walk_switch_cases(&mut self, switch_cases: &Vec<SwitchCase>) {
    for switch_case in switch_cases {
      self.pre_walk_statements(&switch_case.cons)
    }
  }

  fn pre_walk_switch_statement(&mut self, stmt: &SwitchStmt) {
    self.pre_walk_switch_cases(&stmt.cases)
  }

  fn pre_walk_labeled_statement(&mut self, stmt: &LabeledStmt) {
    self.pre_walk_statement(stmt.body.as_ref().into());
  }

  fn pre_walk_if_statement(&mut self, stmt: &IfStmt) {
    self.pre_walk_statement(stmt.cons.as_ref().into());
    if let Some(alter) = &stmt.alt {
      self.pre_walk_statement(alter.as_ref().into());
    }
  }

  pub fn pre_walk_function_declaration(&mut self, decl: MaybeNamedFunctionDecl) {
    if let Some(ident) = decl.ident() {
      self.define_variable(ident.sym.to_string());
    }
  }

  fn pre_walk_for_statement(&mut self, stmt: &ForStmt) {
    if let Some(decl) = stmt.init.as_ref().and_then(|init| init.as_var_decl()) {
      self.pre_walk_statement(Statement::Var(decl))
    }
    self.pre_walk_statement(stmt.body.as_ref().into());
  }

  fn pre_walk_for_of_statement(&mut self, stmt: &ForOfStmt) {
    if stmt.is_await && matches!(self.top_level_scope, super::TopLevelScope::Top) {
      self
        .plugin_drive
        .clone()
        .top_level_for_of_await_stmt(self, stmt);
    }
    if let Some(left) = stmt.left.as_var_decl() {
      self.pre_walk_variable_declaration(left)
    }
    self.pre_walk_statement(stmt.body.as_ref().into())
  }

  pub(super) fn pre_walk_block_statement(&mut self, stmt: &BlockStmt) {
    self.pre_walk_statements(&stmt.stmts);
  }

  fn pre_walk_do_while_statement(&mut self, stmt: &DoWhileStmt) {
    self.pre_walk_statement(stmt.body.as_ref().into());
  }

  fn pre_walk_for_in_statement(&mut self, stmt: &ForInStmt) {
    if let Some(decl) = stmt.left.as_var_decl() {
      self.pre_walk_variable_declaration(decl);
    }
    self.pre_walk_statement(stmt.body.as_ref().into());
  }

  fn pre_walk_variable_declaration(&mut self, decl: &VarDecl) {
    if decl.kind == VarDeclKind::Var {
      self._pre_walk_variable_declaration(decl)
    }
  }

  pub(super) fn _pre_walk_variable_declaration(&mut self, decl: &VarDecl) {
    for declarator in &decl.decls {
      self.pre_walk_variable_declarator(declarator);
      if !self
        .plugin_drive
        .clone()
        .pre_declarator(self, declarator, decl)
        .unwrap_or_default()
      {
        self.enter_pattern(Cow::Borrowed(&declarator.name), |this, ident| {
          this.define_variable(ident.sym.to_string());
        });
      }
    }
  }

  fn _pre_walk_object_pattern(
    &mut self,
    obj_pat: &ObjectPat,
  ) -> Option<FxHashSet<DestructuringAssignmentProperty>> {
    let mut keys = FxHashSet::default();
    for prop in &obj_pat.props {
      match prop {
        ObjectPatProp::KeyValue(prop) => {
          if let Some(ident_key) = prop.key.as_ident() {
            keys.insert(DestructuringAssignmentProperty {
              id: ident_key.sym.clone(),
              range: prop.key.span().into(),
              shorthand: false,
            });
          } else {
            let name = eval::eval_prop_name(&prop.key);
            if let Some(id) = name.and_then(|id| id.as_string()) {
              keys.insert(DestructuringAssignmentProperty {
                id: id.into(),
                range: prop.key.span().into(),
                shorthand: false,
              });
            } else {
              return None;
            }
          }
        }
        ObjectPatProp::Assign(prop) => {
          keys.insert(DestructuringAssignmentProperty {
            id: prop.key.sym.clone(),
            range: prop.key.span().into(),
            shorthand: true,
          });
        }
        ObjectPatProp::Rest(_) => return None,
      };
    }
    Some(keys)
  }

  fn pre_walk_variable_declarator(&mut self, declarator: &VarDeclarator) {
    if self.destructuring_assignment_properties.is_none() {
      return;
    }

    let Some(init) = declarator.init.as_ref() else {
      return;
    };
    let Some(obj_pat) = declarator.name.as_object() else {
      return;
    };
    let Some(keys) = self._pre_walk_object_pattern(obj_pat) else {
      return;
    };

    let destructuring_assignment_properties = self
      .destructuring_assignment_properties
      .as_mut()
      .unwrap_or_else(|| unreachable!());

    if let Some(await_expr) = declarator
      .init
      .as_ref()
      .and_then(|decl| decl.as_await_expr())
    {
      destructuring_assignment_properties.insert(await_expr.arg.span(), keys);
    } else {
      destructuring_assignment_properties.insert(declarator.init.span(), keys);
    }

    if let Some(assign) = init.as_assign() {
      self.pre_walk_assignment_expression(assign);
    }
  }

  pub(super) fn pre_walk_assignment_expression(&mut self, assign: &AssignExpr) {
    if self.destructuring_assignment_properties.is_none() {
      return;
    }
    let Some(left) = assign.left.as_pat().and_then(|pat| pat.as_object()) else {
      return;
    };
    let Some(mut keys) = self._pre_walk_object_pattern(left) else {
      return;
    };

    let destructuring_assignment_properties = self
      .destructuring_assignment_properties
      .as_mut()
      .unwrap_or_else(|| unreachable!());

    if let Some(set) = destructuring_assignment_properties.remove(&assign.span()) {
      keys.extend(set);
    }

    if let Some(await_expr) = assign.right.as_await_expr() {
      destructuring_assignment_properties.insert(await_expr.arg.span(), keys);
    } else {
      destructuring_assignment_properties.insert(assign.right.span(), keys);
    }

    if let Some(right) = assign.right.as_assign() {
      self.pre_walk_assignment_expression(right)
    }
  }
}
