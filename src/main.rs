use hora::core::ann_index::ANNIndex;
use candle_core::{Device, Tensor};

use candle_transformers::models::distilbert::{
    Config, DTYPE, DistilBertModel,
};

use anyhow::{Error as E, Result};
use candle_nn::VarBuilder;
use clap::{Parser, ValueEnum};
use hf_hub::{Repo, RepoType, api::sync::Api};
use std::path::PathBuf;
use tokenizers::Tokenizer;


#[derive(Clone, Debug, Copy, PartialEq, Eq, ValueEnum)]
enum Which {
    #[value(name = "distilbert")]
    DistilBert,
}

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
    fn build_model_and_tokenizer(&self) -> Result<(DistilBertModel, Tokenizer)> {
        let device = candle_core::Device::Cpu;

        let (model_id, revision) = self.resolve_model_and_revision();
        let (config_path, tokenizer_path, weights_path) =
            self.download_model_files(&model_id, &revision)?;

        let config = std::fs::read_to_string(config_path)?;
        let config: Config = serde_json::from_str(&config)?;
        let tokenizer = Tokenizer::from_file(tokenizer_path).map_err(E::msg)?;

        let vb = self.load_variables(&weights_path, &device)?;
        let model = DistilBertModel::load(vb, &config)?;

        Ok((model, tokenizer))
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

    let (model, tokenizer) = args.build_model_and_tokenizer()?;
    let device = &model.device;

    let (token_ids, mask) = prepare_inputs(&args, &tokenizer, device)?;
    let embeddings = model.forward(&token_ids, &mask)?;
    // Apply some avg-pooling by taking the mean embedding value for all tokens (including padding)
    let (_n_sentence, n_tokens, _hidden_size) = embeddings.dims3()?;
    let embeddings = (embeddings.sum(1)? / (n_tokens as f64))?;
    let embeddings = normalize_l2(&embeddings)?.reshape(768)?.to_vec1::<f32>();
    println!("{embeddings:?}");

        let dimension = 768;

        // init index
        let mut index = hora::index::hnsw_idx::HNSWIndex::<f32, usize>::new(
            dimension,
            &hora::index::hnsw_params::HNSWParams::<f32>::default(),
        );

        // add point
        // for (i, sample) in samples.iter().enumerate().take(n) {
        //     index.add(sample, i).unwrap();
        // }
        index.build(hora::core::metrics::Metric::Euclidean).unwrap();

    //     let mut rng = thread_rng();
    //     let target: usize = rng.gen_range(0..n);
    //     // 523 has neighbors: [523, 762, 364, 268, 561, 231, 380, 817, 331, 246]
    //     println!(
    //         "{:?} has neighbors: {:?}",
    //         target,
    //         index.search(&samples[target], 10) // search for k nearest neighbors
    //     );

    Ok(())
}
fn prepare_inputs(args: &Args, tokenizer: &Tokenizer, device: &Device) -> Result<(Tensor, Tensor)> {
    let mut binding = tokenizer.clone();
    let tokenizer_configured = binding
        .with_padding(None)
        .with_truncation(None)
        .map_err(E::msg)?;

    let tokens = tokenizer_configured
        .encode(args.prompt.clone(), true)
        .map_err(E::msg)?
        .get_ids()
        .to_vec();

    let token_ids = Tensor::new(&tokens[..], device)?.unsqueeze(0)?;

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
