use candle_core::{Device, Tensor};
use hora::core::ann_index::ANNIndex;

use candle_transformers::models::distilbert::{Config, DTYPE, DistilBertModel};
use hora::index::hnsw_idx::HNSWIndex;

use anyhow::{Error as E, Result};
use candle_nn::VarBuilder;
use clap::Parser;
use hf_hub::{Repo, RepoType, api::sync::Api};
use std::path::PathBuf;
use tokenizers::Tokenizer;

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Run on CPU rather than on GPU.
    #[arg(long)]
    cpu: bool,

    /// The model to use, check out available models: https://huggingface.co/models?library=sentence-transformers&sort=trending
    #[arg(long)]
    model_id: Option<String>,

    /// Revision or branch
    #[arg(long)]
    revision: Option<String>,

    /// When set, compute embeddings for this prompt.
    #[arg(long)]
    prompt: String,
}

impl Args {
    fn build_embed(&self) -> Result<Embed> {
        let device = if self.cpu {
            candle_core::Device::Cpu
        } else {
            candle_core::Device::new_cuda(0)?
        };

        let (model_id, revision) = self.resolve_model_and_revision();
        let (config_path, tokenizer_path, weights_path) =
            self.download_model_files(&model_id, &revision)?;

        let config = std::fs::read_to_string(config_path)?;
        let config: Config = serde_json::from_str(&config)?;
        let tokenizer = Tokenizer::from_file(tokenizer_path).map_err(E::msg)?;

        let vb = self.load_variables(&weights_path, &device)?;
        let model = DistilBertModel::load(vb, &config)?;

        Ok(Embed { model, tokenizer })
    }

    fn resolve_model_and_revision(&self) -> (String, String) {
        let default_model = "distilbert-base-uncased".to_string();
        let default_revision = "main".to_string();

        match (self.model_id.clone(), self.revision.clone()) {
            (Some(model_id), Some(revision)) => (model_id, revision),
            (Some(model_id), None) => (model_id, default_revision),
            (None, Some(revision)) => (default_model, revision),
            (None, None) => (default_model, default_revision),
        }
    }

    fn download_model_files(
        &self,
        model_id: &str,
        revision: &str,
    ) -> Result<(PathBuf, PathBuf, PathBuf)> {
        let repo = Repo::with_revision(model_id.to_string(), RepoType::Model, revision.to_string());
        let api = Api::new()?;
        let api = api.repo(repo);

        let config = api.get("config.json")?;
        let tokenizer = api.get("tokenizer.json")?;
        let weights = api.get("model.safetensors")?;

        Ok((config, tokenizer, weights))
    }

    fn load_variables(&self, weights_path: &PathBuf, device: &Device) -> Result<VarBuilder> {
        Ok(unsafe { VarBuilder::from_mmaped_safetensors(&[weights_path], DTYPE, device)? })
    }
}

fn main() -> Result<()> {
    let args = Args::parse();
    let prompt = args.prompt.clone();
    let embed = args.build_embed()?;

    let dimension = 768;

    let mut index = HNSWIndex::<f32, usize>::new(
        dimension,
        &hora::index::hnsw_params::HNSWParams::<f32>::default(),
    );

    let strings = ["lol"];

    for (i, s) in strings.iter().enumerate() {
        index.add(&embed.embed(s)?, i).map_err(E::msg)?;
    }
    index
        .build(hora::core::metrics::Metric::Euclidean)
        .map_err(E::msg)?;

    let target = embed.embed(&prompt)?;

    let k = 2;

    // search for k nearest neighbors
    let nn: Vec<&str> = index
        .search(&target, k)
        .into_iter()
        .map(|i| strings[i])
        .collect();
    println!("target has neighbors: {:?}", nn);

    Ok(())
}

pub struct Embed {
    model: DistilBertModel,
    tokenizer: Tokenizer,
}

impl Embed {
    fn embed(&self, prompt: &str) -> Result<Vec<f32>> {
        let (token_ids, mask) = prepare_inputs(prompt, &self.tokenizer, &self.model.device)?;
        let embeddings = self.model.forward(&token_ids, &mask)?;
        let (_n_sentence, n_tokens, _hidden_size) = embeddings.dims3()?;
        let embeddings = (embeddings.sum(1)? / (n_tokens as f64))?;
        let embeddings = normalize_l2(&embeddings)?.reshape(768)?.to_vec1::<f32>()?;
        Ok(embeddings)
    }
}
fn prepare_inputs(
    prompt: &str,
    tokenizer: &Tokenizer,
    device: &Device,
) -> Result<(Tensor, Tensor)> {
    let mut binding = tokenizer.clone();
    let tokenizer_configured = binding
        .with_padding(None)
        .with_truncation(None)
        .map_err(E::msg)?;

    let tokens = tokenizer_configured
        .encode(prompt, true)
        .map_err(E::msg)?
        .get_ids()
        .to_vec();

    let token_ids = Tensor::new(tokens.as_slice(), device)?.unsqueeze(0)?;

    let mask = attention_mask(tokens.len(), device)?;
    println!("token_ids: {:?}", token_ids.to_vec2::<u32>()?);

    Ok((token_ids, mask))
}

fn attention_mask(size: usize, device: &Device) -> Result<Tensor> {
    let mask: Vec<_> = (0..size)
        .flat_map(|i| (0..size).map(move |j| u8::from(j > i)))
        .collect();
    Ok(Tensor::from_slice(&mask, (size, size), device)?)
}

pub fn normalize_l2(v: &Tensor) -> Result<Tensor> {
    Ok(v.broadcast_div(&v.sqr()?.sum_keepdim(1)?.sqrt()?)?)
}
