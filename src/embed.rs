use super::Args;
use anyhow::{Error as E, Result};
use candle_core::Device;
use candle_core::Tensor;
use candle_nn::VarBuilder;
use candle_transformers::models::bert::{BertModel, Config, DTYPE};
use hf_hub::Repo;
use hf_hub::RepoType;
use hf_hub::api::sync::Api;
use std::path::PathBuf;
use tokenizers::Tokenizer;
pub struct Embed {
    model: BertModel,
    tokenizer: Tokenizer,
}

impl Embed {
    pub(crate) fn new(args: Args) -> Result<Self> {
        let device = if args.cpu {
            candle_core::Device::Cpu
        } else {
            candle_core::Device::new_cuda(0)?
        };

        let (model_id, revision) = args.resolve_model_and_revision();
        let (config_path, tokenizer_path, weights_path) =
            Self::download_model_files(&model_id, &revision)?;

        let config = std::fs::read_to_string(config_path)?;
        let config: Config = serde_json::from_str(&config)?;
        let tokenizer = Tokenizer::from_file(tokenizer_path).map_err(E::msg)?;

        let vb = unsafe { VarBuilder::from_mmaped_safetensors(&[weights_path], DTYPE, &device)? };
        let model = BertModel::load(vb, &config)?;

        Ok(Embed { model, tokenizer })
    }

    fn download_model_files(model_id: &str, revision: &str) -> Result<(PathBuf, PathBuf, PathBuf)> {
        let repo = Repo::with_revision(model_id.to_string(), RepoType::Model, revision.to_string());
        let api = Api::new()?.repo(repo);

        let config = api.get("config.json")?;
        let tokenizer = api.get("tokenizer.json")?;
        let weights = api.get("model.safetensors")?;

        Ok((config, tokenizer, weights))
    }

    pub(crate) fn embed(&mut self, prompt: &str) -> Result<Vec<f32>> {
        let tokenizer = self
            .tokenizer
            .with_padding(None)
            .with_truncation(None)
            .map_err(E::msg)?;

        let tokens = tokenizer
            .encode(prompt, true)
            .map_err(E::msg)?
            .get_ids()
            .to_vec();

        let token_ids = Tensor::new(tokens.as_slice(), &self.model.device)?.unsqueeze(0)?;
        let token_type_ids = token_ids.zeros_like()?;

        let embeddings = self.model.forward(&token_ids, &token_type_ids, None)?;
        let (_n_sentence, n_tokens, _hidden_size) = embeddings.dims3()?;
        let embeddings = (embeddings.sum(1)? / (n_tokens as f64))?;
        let embeddings = normalize_l2(&embeddings)?.reshape(384)?.to_vec1::<f32>()?;
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
    // println!("token_ids: {:?}", token_ids.to_vec2::<u32>()?);

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
