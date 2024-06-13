use std::marker::PhantomData;

use super::{Factory, OperationTrait, ReadableOperationTrait, TransformOperationTrait};
use crate::utils::YamlWrapper;
use anyhow::Result;
use async_trait::async_trait;

#[async_trait]
pub trait ReadableBuilderTrait: Send + Sync {
	fn get_id(&self) -> &'static str;
	fn get_docs(&self) -> String;
	async fn build(&self, yaml: YamlWrapper, factory: &Factory) -> Result<Box<dyn OperationTrait>>;
}

#[async_trait]
pub trait TransformBuilderTrait: Send + Sync {
	fn get_id(&self) -> &'static str;
	fn get_docs(&self) -> String;
	async fn build(
		&self,
		yaml: YamlWrapper,
		reader: Box<dyn OperationTrait>,
		factory: &Factory,
	) -> Result<Box<dyn OperationTrait>>;
}

#[derive(Clone)]
pub struct ReadableBuilder<T>
where
	T: ReadableOperationTrait + 'static,
{
	_type: PhantomData<T>,
}

#[async_trait]
impl<T> ReadableBuilderTrait for ReadableBuilder<T>
where
	T: ReadableOperationTrait + 'static,
{
	fn get_docs(&self) -> String {
		T::get_docs()
	}
	fn get_id(&self) -> &'static str {
		T::get_id()
	}
	async fn build(&self, yaml: YamlWrapper, factory: &Factory) -> Result<Box<dyn OperationTrait>> {
		Ok(Box::new(T::new(yaml, factory).await?))
	}
}

impl<T> ReadableBuilder<T>
where
	T: ReadableOperationTrait + 'static,
{
	pub fn new() -> Box<Self> {
		Box::new(Self { _type: PhantomData })
	}
}

#[derive(Clone)]
pub struct TransformBuilder<T>
where
	T: TransformOperationTrait + 'static,
{
	_type: PhantomData<T>,
}

#[async_trait]
impl<T> TransformBuilderTrait for TransformBuilder<T>
where
	T: TransformOperationTrait + 'static,
{
	fn get_id(&self) -> &'static str {
		T::get_id()
	}
	fn get_docs(&self) -> String {
		T::get_docs()
	}
	async fn build(
		&self,
		yaml: YamlWrapper,
		reader: Box<dyn OperationTrait>,
		factory: &Factory,
	) -> Result<Box<dyn OperationTrait>> {
		Ok(Box::new(T::new(yaml, reader, factory).await?))
	}
}

impl<T> TransformBuilder<T>
where
	T: TransformOperationTrait + 'static,
{
	pub fn new() -> Box<Self> {
		Box::new(Self { _type: PhantomData })
	}
}
