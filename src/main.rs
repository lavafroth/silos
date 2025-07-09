use anyhow::{Context, Error as E, Result, bail};
use clap::Parser;
use hora::core::{ann_index::ANNIndex, metrics::Metric::Euclidean};
use hora::index::hnsw_idx::HNSWIndex;
use kdl::KdlDocument;
use state::State;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;
use tower_lsp::{LspService, Server};

mod args;
mod embed;
mod lsp;
mod state;
mod mutation;

fn path_to_parent_base(p: &std::path::Path) -> Result<String> {
    let Some(parent) = p
        .parent()
        .and_then(|v| v.file_name())
        .and_then(|v| v.to_str())
        .map(|v| v.to_string())
    else {
        bail!("failed to parse snippets path, make sure the directory structure is valid");
    };
    Ok(parent)
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt::init();
    let args = args::Args::parse();
    let (model_id, revision) = args.resolve_model_and_revision();
    let embed = embed::Embed::new(args.gpu, &model_id, &revision)?;
    let mut dict = HashMap::default();
    let dimensions = 384;

    let paths = glob::glob("./snippets/v1/*/*.kdl")?;
    for path in paths {
        let path = path?;
        let parent = path_to_parent_base(&path)?;

        let current_lang_index = dict
            .entry(parent)
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

    for index in dict.values_mut() {
        index
            .build(hora::core::metrics::Metric::Euclidean)
            .map_err(E::msg)?;
    }

    // v2
    let paths = glob::glob("./snippets/v2/*/*.kdl")?;
    let mut v2_dict = HashMap::new();
    let mut v2_mutations_collection = vec![];
    for (i, path) in paths.enumerate() {
        let path = path?;
        let parent = path_to_parent_base(&path)?;

        let mutations = mutation::from_path(path)?;
        let current_lang_index = v2_dict
            .entry(parent)
            .or_insert_with(|| HNSWIndex::new(dimensions, &Default::default()));

        current_lang_index
            .add(&embed.embed(&mutations.description)?, i)
            .map_err(E::msg)?;
        v2_mutations_collection.push(mutations);
    }

    for index in v2_dict.values_mut() {
        index.build(Euclidean).map_err(E::msg)?;
    }

    let appstate = State {
        embed,
        generate: state::Generate { dict },
        refactor: state::Refactor {
            dict: v2_dict,
            mutations_collection: v2_mutations_collection,
        },
    };

    let stdin = tokio::io::stdin();
    let stdout = tokio::io::stdout();

    let (service, socket) = LspService::new(|client| lsp::Backend {
        client,
        body: Arc::new(Mutex::new(String::default())),
        appstate
    });
    Server::new(stdin, stdout, socket).serve(service).await;
    Ok(())
}
