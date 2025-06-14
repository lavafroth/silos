use actix_web::{App, HttpServer, web};
use anyhow::{Error as E, Result};
use clap::Parser;
use hora::core::ann_index::ANNIndex;
use hora::index::hnsw_idx::HNSWIndex;
use std::collections::{BTreeMap, HashMap};
mod embed;
mod v1;

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

#[actix_web::main]
async fn main() -> Result<()> {
    let args = Args::parse();

    let mut embed = embed::Embed::new(args)?;

    let mut lang_map = BTreeMap::default();

    let paths = glob::glob("./snippets/v1/**/*.json")?;
    for path in paths {
        let path = path?;
        let parent = path
            .components()
            .rev()
            .nth(1)
            .unwrap()
            .as_os_str()
            .to_string_lossy()
            .to_string();

        let current_lang_index = lang_map.entry(parent).or_insert_with(|| {
            let dimension = 384;
            let params = hora::index::hnsw_params::HNSWParams::<f32>::default();

            HNSWIndex::<f32, String>::new(dimension, &params)
        });

        let snippet: v1::api::SnippetOnDisk =
            serde_json::from_str(&std::fs::read_to_string(path)?)?;
        current_lang_index
            .add(&embed.embed(&snippet.desc)?, snippet.body)
            .map_err(E::msg)?;
    }

    let mut appstate = HashMap::default();

    for (k, mut index) in lang_map.into_iter() {
        index
            .build(hora::core::metrics::Metric::Euclidean)
            .map_err(E::msg)?;
        appstate.insert(k, index);
    }
    let appstate = v1::api::AppState {
        dict: appstate,
        embed,
    };

    let appstate_wrapped = web::Data::new(appstate.build());

    HttpServer::new(move || {
        App::new()
            .app_data(appstate_wrapped.clone())
            .service(v1::api::get_snippet)
            .service(v1::api::add_snippet)
    })
    .bind(("127.0.0.1", 8000))?
    .run()
    .await
    .map_err(E::from)
}
