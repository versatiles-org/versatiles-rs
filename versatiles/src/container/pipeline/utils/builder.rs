use super::{
	ComposerOperationTrait, Factory, OperationTrait, ReaderOperationTrait, TransformerOperationTrait,
};
use crate::utils::YamlWrapper;
use anyhow::Result;
use async_trait::async_trait;
use std::marker::PhantomData;

#[derive(Clone)]
pub struct Builder<T>
where
	T: 'static,
{
	_type: PhantomData<T>,
}

impl<T> Builder<T>
where
	T: 'static,
{
	pub fn new() -> Box<Self> {
		Box::new(Self { _type: PhantomData })
	}
}

#[async_trait]
pub trait BuilderTrait: Send + Sync {
	fn get_id(&self) -> &'static str;
	fn get_docs(&self) -> String;
}

#[async_trait]
impl<T> BuilderTrait for Builder<T>
where
	T: OperationTrait + 'static,
{
	fn get_id(&self) -> &'static str {
		T::get_id()
	}

	fn get_docs(&self) -> String {
		T::get_docs()
	}
}

#[async_trait]
pub trait ReaderBuilderTrait: BuilderTrait + Send + Sync {
	async fn build(&self, yaml: YamlWrapper, factory: &Factory) -> Result<Box<dyn OperationTrait>>;
}

#[async_trait]
impl<T> ReaderBuilderTrait for Builder<T>
where
	T: ReaderOperationTrait + 'static,
{
	async fn build(&self, yaml: YamlWrapper, factory: &Factory) -> Result<Box<dyn OperationTrait>> {
		Ok(Box::new(T::new(yaml, factory).await?))
	}
}

#[async_trait]
pub trait TransformerBuilderTrait: BuilderTrait + Send + Sync {
	async fn build(
		&self,
		yaml: YamlWrapper,
		source: Box<dyn OperationTrait>,
		factory: &Factory,
	) -> Result<Box<dyn OperationTrait>>;
}

#[async_trait]
impl<T> TransformerBuilderTrait for Builder<T>
where
	T: TransformerOperationTrait + 'static,
{
	async fn build(
		&self,
		yaml: YamlWrapper,
		source: Box<dyn OperationTrait>,
		factory: &Factory,
	) -> Result<Box<dyn OperationTrait>> {
		Ok(Box::new(T::new(yaml, source, factory).await?))
	}
}

#[async_trait]
pub trait ComposerBuilderTrait: BuilderTrait + Send + Sync {
	async fn build(
		&self,
		yaml: YamlWrapper,
		sources: Vec<Box<dyn OperationTrait>>,
		factory: &Factory,
	) -> Result<Box<dyn OperationTrait>>;
}

#[async_trait]
impl<T> ComposerBuilderTrait for Builder<T>
where
	T: ComposerOperationTrait + 'static,
{
	async fn build(
		&self,
		yaml: YamlWrapper,
		sources: Vec<Box<dyn OperationTrait>>,
		factory: &Factory,
	) -> Result<Box<dyn OperationTrait>> {
		Ok(Box::new(T::new(yaml, sources, factory).await?))
	}
}
