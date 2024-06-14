use super::{Factory, OperationTrait, ReaderOperationTrait, TransformerOperationTrait};
use crate::utils::YamlWrapper;
use anyhow::Result;
use async_trait::async_trait;
use std::marker::PhantomData;

#[async_trait]
pub trait BuilderTrait: Send + Sync {
	fn get_id(&self) -> &'static str;
	fn get_docs(&self) -> String;
}

#[async_trait]
pub trait ReaderBuilderTrait: BuilderTrait + Send + Sync {
	async fn build(&self, yaml: YamlWrapper, factory: &Factory) -> Result<Box<dyn OperationTrait>>;
}

#[async_trait]
pub trait TransformerBuilderTrait: BuilderTrait + Send + Sync {
	async fn build(
		&self,
		yaml: YamlWrapper,
		reader: Box<dyn OperationTrait>,
		factory: &Factory,
	) -> Result<Box<dyn OperationTrait>>;
}

#[derive(Clone)]
pub struct ReaderBuilder<T>
where
	T: ReaderOperationTrait + 'static,
{
	_type: PhantomData<T>,
}

#[async_trait]
impl<T> BuilderTrait for ReaderBuilder<T>
where
	T: ReaderOperationTrait + 'static,
{
	fn get_docs(&self) -> String {
		T::get_docs()
	}
	fn get_id(&self) -> &'static str {
		T::get_id()
	}
}

#[async_trait]
impl<T> ReaderBuilderTrait for ReaderBuilder<T>
where
	T: ReaderOperationTrait + 'static,
{
	async fn build(&self, yaml: YamlWrapper, factory: &Factory) -> Result<Box<dyn OperationTrait>> {
		Ok(Box::new(T::new(yaml, factory).await?))
	}
}

impl<T> ReaderBuilder<T>
where
	T: ReaderOperationTrait + 'static,
{
	pub fn new() -> Box<Self> {
		Box::new(Self { _type: PhantomData })
	}
}

#[derive(Clone)]
pub struct TransformerBuilder<T>
where
	T: TransformerOperationTrait + 'static,
{
	_type: PhantomData<T>,
}

#[async_trait]
impl<T> BuilderTrait for TransformerBuilder<T>
where
	T: TransformerOperationTrait + 'static,
{
	fn get_id(&self) -> &'static str {
		T::get_id()
	}
	fn get_docs(&self) -> String {
		T::get_docs()
	}
}

#[async_trait]
impl<T> TransformerBuilderTrait for TransformerBuilder<T>
where
	T: TransformerOperationTrait + 'static,
{
	async fn build(
		&self,
		yaml: YamlWrapper,
		reader: Box<dyn OperationTrait>,
		factory: &Factory,
	) -> Result<Box<dyn OperationTrait>> {
		Ok(Box::new(T::new(yaml, reader, factory).await?))
	}
}

impl<T> TransformerBuilder<T>
where
	T: TransformerOperationTrait + 'static,
{
	pub fn new() -> Box<Self> {
		Box::new(Self { _type: PhantomData })
	}
}
