use anyhow::{Error as E, Result};
use candle_core::Device;
use candle_core::Tensor;
use candle_nn::VarBuilder;
use candle_transformers::models::bert::{BertModel, Config, DTYPE};
use hf_hub::Repo;
use hf_hub::RepoType;
use hf_hub::api::sync::Api;
use std::path::PathBuf;
use tokenizers::DecoderWrapper;
use tokenizers::ModelWrapper;
use tokenizers::NormalizerWrapper;
use tokenizers::PostProcessorWrapper;
use tokenizers::PreTokenizerWrapper;
use tokenizers::Tokenizer;
use tokenizers::TokenizerImpl;

pub struct Embed {
    model: BertModel,
    pub hidden_size: usize,
    tokenizer: TokenizerImpl<
        ModelWrapper,
        NormalizerWrapper,
        PreTokenizerWrapper,
        PostProcessorWrapper,
        DecoderWrapper,
    >,
}

impl Embed {
    pub(crate) fn new(gpu: Option<usize>, model_id: &str, revision: &str) -> Result<Self> {
        let device = if let Some(gpu_dev) = gpu {
            Device::new_cuda(gpu_dev)?
        } else {
            Device::Cpu
        };

        let (config_path, tokenizer_path, weights_path) =
            Self::download_model_files(model_id, revision)?;

        let config = std::fs::read_to_string(config_path)?;
        let config: Config = serde_json::from_str(&config)?;
        let mut tokenizer = Tokenizer::from_file(tokenizer_path).map_err(E::msg)?;

        let vb = unsafe { VarBuilder::from_mmaped_safetensors(&[weights_path], DTYPE, &device)? };
        let model = BertModel::load(vb, &config)?;

        let tokenizer = tokenizer
            .with_padding(None)
            .with_truncation(None)
            .map_err(E::msg)?
            .clone();

        Ok(Embed {
            model,
            tokenizer,
            hidden_size: config.hidden_size,
        })
    }

    fn download_model_files(model_id: &str, revision: &str) -> Result<(PathBuf, PathBuf, PathBuf)> {
        let repo = Repo::with_revision(model_id.to_string(), RepoType::Model, revision.to_string());
        let api = Api::new()?.repo(repo);

        let config = api.get("config.json")?;
        let tokenizer = api.get("tokenizer.json")?;
        let weights = api.get("model.safetensors")?;

        Ok((config, tokenizer, weights))
    }

    pub(crate) fn embed(&self, prompt: &str) -> Result<Vec<f32>> {
        let tokens = self
            .tokenizer
            .encode(prompt, true)
            .map_err(E::msg)?
            .get_ids()
            .to_vec();

        let token_ids = Tensor::new(tokens.as_slice(), &self.model.device)?.unsqueeze(0)?;
        let token_type_ids = token_ids.zeros_like()?;

        let embeddings = self.model.forward(&token_ids, &token_type_ids, None)?;
        let embeddings = normalize_l2(&embeddings.sum(1)?)?
            .reshape(self.hidden_size)?
            .to_vec1::<f32>()?;
        Ok(embeddings)
    }
}

pub fn normalize_l2(v: &Tensor) -> Result<Tensor> {
    Ok(v.broadcast_div(&v.sqr()?.sum_keepdim(1)?.sqrt()?)?)
}
