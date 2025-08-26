use anyhow::{Context, Error as E, Result};
use clap::Parser;
use hora::core::{ann_index::ANNIndex, metrics::Metric::Euclidean};
use hora::index::hnsw_idx::HNSWIndex;
use kdl::KdlDocument;
use state::{State, dump_expression};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;
use tower_lsp::{LspService, Server};

mod args;
mod embed;
mod lsp;
mod mutation;
mod sources;
mod state;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt::init();
    let args = match args::Cli::parse().command {
        args::Command::Ast(ast) => {
            match ast {
                args::Ast::DumpExpression(source_file) => {
                    println!("{}", dump_expression(&source_file.path)?);
                }
                args::Ast::ShowCaptures(show_captures) => {
                    let source = std::fs::read_to_string(&show_captures.path).unwrap();
                    let source_bytes = source.as_bytes();
                    let extension = show_captures.path.extension().unwrap().to_str().unwrap();
                    let langfn = state::Refactor::get_lang(extension).unwrap();
                    let mut parser = tree_sitter::Parser::new();
                    parser.set_language(&langfn).unwrap();
                    let tree = parser.parse(source_bytes, None).unwrap();
                    let root_node = tree.root_node();
                    let cooked = mutation::query(
                        root_node,
                        &show_captures.expression,
                        &langfn,
                        source_bytes,
                    );
                    println!("{:#?}", cooked);
                }
            }
            return Ok(());
        }
        args::Command::Lsp(lsp) => lsp,
    };

    let (model_id, revision) = args.resolve_model_and_revision();

    let embed = embed::Embed::new(args.gpu, &model_id, &revision)?;
    let mut dict = HashMap::default();
    let dimensions = embed.hidden_size;

    for (language, paths) in sources::rule_files(args.snippets.join("generate"))? {
        for path in paths {
            let current_lang_index = dict
                .entry(language.clone())
                .or_insert_with(|| HNSWIndex::new(dimensions, &Default::default()));

            let doc_str = std::fs::read_to_string(&path)?;
            let doc: KdlDocument = doc_str
                .parse()
                .context(format!("failed to parse KDL: {}", path.display()))?;

            let Some(desc) = doc.get_arg("desc").and_then(|v| v.as_string()) else {
                continue;
            };
            let Some(body) = doc.get_arg("body").and_then(|v| v.as_string()) else {
                continue;
            };
            current_lang_index
                .add(&embed.embed(desc)?, body.to_string())
                .map_err(E::msg)?;
        }
    }

    for index in dict.values_mut() {
        index
            .build(hora::core::metrics::Metric::Euclidean)
            .map_err(E::msg)?;
    }

    let mut refactor_dict = HashMap::new();
    let mut mutations_collection = vec![];
    for (language, paths) in sources::rule_files(args.snippets.join("refactor"))? {
        for path in paths {
            let mutations = mutation::from_path(path)?;
            let current_lang_index = refactor_dict
                .entry(language.clone())
                .or_insert_with(|| HNSWIndex::new(dimensions, &Default::default()));

            current_lang_index
                .add(
                    &embed.embed(&mutations.description)?,
                    mutations_collection.len(),
                )
                .map_err(E::msg)?;
            mutations_collection.push(mutations);
        }
    }

    for index in refactor_dict.values_mut() {
        index.build(Euclidean).map_err(E::msg)?;
    }

    let appstate = State::new(
        embed,
        state::Generate { dict },
        state::Refactor {
            dict: refactor_dict,
            mutations_collection,
        },
    );

    let stdin = tokio::io::stdin();
    let stdout = tokio::io::stdout();

    let (service, socket) = LspService::new(|client| lsp::Backend {
        client,
        body: Arc::new(Mutex::new(HashMap::default())),
        appstate,
    });
    Server::new(stdin, stdout, socket).serve(service).await;
    Ok(())
}
