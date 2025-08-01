use std::sync::Arc;

use rspack_core::{Compilation, ErrorSpan};
use rspack_error::{
  DiagnosticKind, IntoTWithDiagnosticArray, Result, TWithDiagnosticArray,
  ToStringResultToRspackResultExt,
};
use swc_core::common::{FileName, FilePathMapping, GLOBALS, SourceFile, SourceMap, sync::Lrc};
use swc_html::{
  ast::Document,
  codegen::{
    CodeGenerator, CodegenConfig, Emit,
    writer::basic::{BasicHtmlWriter, BasicHtmlWriterConfig},
  },
  parser::{error::Error, parse_file_as_document, parser::ParserConfig},
};
use swc_html_minifier::{
  MinifyCss, minify_document_with_custom_css_minifier, option::MinifyOptions,
};

use crate::config::HtmlRspackPluginOptions;

pub struct HtmlCompiler<'a> {
  config: &'a HtmlRspackPluginOptions,
}

impl<'a> HtmlCompiler<'a> {
  pub fn new(config: &'a HtmlRspackPluginOptions) -> Self {
    Self { config }
  }

  pub fn parse_file(&self, path: &str, source: String) -> Result<TWithDiagnosticArray<Document>> {
    let cm = Lrc::new(SourceMap::new(FilePathMapping::empty()));
    let fm = cm.new_source_file(Arc::new(FileName::Custom(path.to_string())), source.clone());

    let mut errors = vec![];
    let document = parse_file_as_document(fm.as_ref(), ParserConfig::default(), &mut errors);
    let diagnostics: Vec<rspack_error::Diagnostic> = errors
      .into_iter()
      .flat_map(|error| vec![html_parse_error_to_traceable_error(error, &fm).into()])
      .collect();
    document
      .map(|doc| doc.with_diagnostic(diagnostics))
      .map_err(|e| html_parse_error_to_traceable_error(e, &fm))
  }

  pub fn codegen(&self, ast: &mut Document, compilation: &Compilation) -> Result<String> {
    let writer_config = BasicHtmlWriterConfig::default();
    let minify = self.config.minify.unwrap_or(matches!(
      compilation.options.mode,
      rspack_core::Mode::Production
    ));
    let codegen_config = CodegenConfig {
      minify,
      quotes: Some(true),
      tag_omission: Some(false),
      ..Default::default()
    };
    if minify {
      // Minify can't leak to user land because it doesn't implement `ToNapiValue` Trait
      GLOBALS.set(&Default::default(), || {
        minify_document_with_custom_css_minifier(
          ast,
          &MinifyOptions::<()>::default(),
          &NoopCssMinifier,
        );
      })
    }

    let mut output = String::new();
    let wr = BasicHtmlWriter::new(&mut output, None, writer_config);
    let mut r#gen = CodeGenerator::new(wr, codegen_config);

    r#gen.emit(ast).to_rspack_result()?;
    Ok(output)
  }
}

pub fn html_parse_error_to_traceable_error(error: Error, fm: &SourceFile) -> rspack_error::Error {
  let message = error.message();
  let error = error.into_inner();
  let span: ErrorSpan = error.0.into();
  let traceable_error = rspack_error::TraceableError::from_source_file(
    fm,
    span.start as usize,
    span.end as usize,
    "HTML parse error".to_string(),
    message.to_string(),
  )
  .with_kind(DiagnosticKind::Html);
  //Use this `Error` conversion could avoid eagerly clone source file.
  traceable_error.into()
}

struct NoopCssMinifier;

impl MinifyCss for NoopCssMinifier {
  type Options = ();

  fn minify_css(
    &self,
    _options: &swc_html_minifier::option::MinifyCssOption<Self::Options>,
    data: String,
    _mode: swc_html_minifier::CssMinificationMode,
  ) -> Option<String> {
    Some(data)
  }
}
