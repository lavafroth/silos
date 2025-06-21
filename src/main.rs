use actix_web::{App, HttpServer, web};
use anyhow::{Context, Error as E, Result, bail};
use clap::Parser;
use hora::core::ann_index::ANNIndex;
use hora::index::hnsw_idx::HNSWIndex;
use kdl::KdlDocument;
use state::State;
use std::collections::HashMap;
use tracing::info_span;

mod embed;
mod state;
mod v1;
mod v2;

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Run on the Nth GPU device.
    #[arg(long)]
    gpu: Option<usize>,

    /// The model to use, check out available models: https://huggingface.co/models?library=sentence-transformers&sort=trending
    #[arg(long)]
    model_id: Option<String>,

    /// Revision or branch.
    #[arg(long)]
    revision: Option<String>,

    /// The port for the API to listen on
    #[arg(long, default_value = "8000")]
    port: u16,
}

impl Args {
    fn resolve_model_and_revision(&self) -> (String, String) {
        let default_model = "sentence-transformers/all-MiniLM-L6-v2".to_string();
        let default_revision = "refs/pr/21".to_string();

        match (self.model_id.clone(), self.revision.clone()) {
            (Some(model_id), Some(revision)) => (model_id, revision),
            (Some(model_id), None) => (model_id, "main".to_owned()),
            (None, Some(revision)) => (default_model, revision),
            (None, None) => (default_model, default_revision),
        }
    }
}

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

#[actix_web::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt::init();
    let args = Args::parse();
    let port = args.port;
    let mut embed = embed::Embed::new(args)?;
    let mut dict = HashMap::default();

    let paths = glob::glob("./snippets/v1/*/*.kdl")?;
    for path in paths {
        let path = path?;
        let parent = path_to_parent_base(&path)?;

        let current_lang_index = dict.entry(parent).or_insert_with(|| {
            let dimension = 384;
            let params = hora::index::hnsw_params::HNSWParams::<f32>::default();

            HNSWIndex::<f32, String>::new(dimension, &params)
        });

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

    // v2 stuff
    let paths = glob::glob("./snippets/v2/*/*.kdl")?;
    let mut v2_dict = HashMap::new();
    let mut v2_mutations_collection = vec![];
    for (i, path) in paths.enumerate() {
        let path = path?;
        let parent = path_to_parent_base(&path)?;

        let mutations = v2::mutation::from_path(path)?;
        let current_lang_index = v2_dict.entry(parent).or_insert_with(|| {
            let dimension = 384;
            let params = hora::index::hnsw_params::HNSWParams::<f32>::default();

            HNSWIndex::<f32, usize>::new(dimension, &params)
        });

        current_lang_index
            .add(&embed.embed(&mutations.description)?, i)
            .map_err(E::msg)?;
        v2_mutations_collection.push(mutations);
    }

    for index in v2_dict.values_mut() {
        index
            .build(hora::core::metrics::Metric::Euclidean)
            .map_err(E::msg)?;
    }

    let appstate = State {
        embed,
        v1: v1::api::State { dict },
        v2: v2::api::State {
            dict: v2_dict,
            mutations_collection: v2_mutations_collection,
        },
    };

    let appstate_wrapped = web::Data::new(appstate.build());

    HttpServer::new(move || {
        App::new()
            .app_data(appstate_wrapped.clone())
            .service(v1::api::get_snippet)
            .service(v1::api::add_snippet)
            .service(v2::api::get_snippet)
    })
    .bind(("127.0.0.1", port))?
    .run()
    .await
    .map_err(E::from)
}
