use super::definition::{
	ArgKind, Child, ChildMode, DecodeMode, ExtraKind, Field, FieldAttrs, NewType, Struct,
	StructBuilder,
};
use proc_macro2::{Span, TokenStream};
use quote::{format_ident, quote, ToTokens};
use syn::ext::IdentExt;

pub(crate) struct Common<'a> {
	pub object: &'a Struct,
	pub ctx: &'a syn::Ident,
	pub span_type: &'a TokenStream,
}

fn child_can_partial(child: &Child) -> bool {
	use ChildMode::*;

	child.option || matches!(child.mode, Bool | Flatten)
}

pub fn emit_struct(s: &Struct, named: bool) -> syn::Result<TokenStream> {
	let s_name = &s.ident;
	let node = syn::Ident::new("node", Span::mixed_site());
	let ctx = syn::Ident::new("ctx", Span::mixed_site());
	let children = syn::Ident::new("children", Span::mixed_site());

	let (_, type_gen, _) = s.generics.split_for_impl();
	let mut common_generics = s.generics.clone();
	let span_ty;
	if let Some(ty) = s.trait_props.span_type.as_ref() {
		span_ty = quote!(#ty);
	} else {
		if common_generics.params.is_empty() {
			common_generics.lt_token = Some(Default::default());
			common_generics.gt_token = Some(Default::default());
		}
		common_generics.params.push(syn::parse2(quote!(S)).unwrap());
		span_ty = quote!(S);
		common_generics
			.make_where_clause()
			.predicates
			.push(syn::parse2(quote!(S: ::knuffel::traits::ErrorSpan)).unwrap());
	};
	let trait_gen = quote!(<#span_ty>);
	let (impl_gen, _, bounds) = common_generics.split_for_impl();

	let common = Common {
		object: s,
		ctx: &ctx,
		span_type: &span_ty,
	};

	let decode_specials = decode_specials(&common, &node)?;
	let decode_args = decode_args(&common, &node)?;
	let decode_props = decode_props(&common, &node)?;
	let decode_children_normal = decode_children(&common, &children, Some(quote!(#node.span())))?;
	let assign_extra = assign_extra(&common)?;

	let all_fields = s.all_fields();
	let struct_val = if named {
		let assignments = all_fields.iter().map(|f| f.as_assign_pair().unwrap());
		quote!(#s_name { #(#assignments,)* })
	} else {
		let mut fields = all_fields
			.iter()
			.map(|f| (f.as_index().unwrap(), &f.tmp_name))
			.collect::<Vec<_>>();
		fields.sort_by_key(|(idx, _)| *idx);
		assert_eq!(
			fields.iter().map(|(idx, _)| *idx).collect::<Vec<_>>(),
			(0..fields.len()).collect::<Vec<_>>(),
			"all tuple structure fields should be filled in"
		);
		let assignments = fields.iter().map(|(_, v)| v);
		quote! { #s_name(#(#assignments),*) }
	};
	let mut extra_traits = Vec::new();
	let partial_compatible = s.spans.is_empty()
		&& s.node_names.is_empty()
		&& s.type_names.is_empty()
		&& !s.has_arguments
		&& (s.properties.iter().all(|x| x.option || x.flatten) && s.var_props.is_none())
		&& (s.children.iter().all(child_can_partial) && s.var_children.is_none());
	if partial_compatible {
		let node = syn::Ident::new("node", Span::mixed_site());
		let name = syn::Ident::new("name", Span::mixed_site());
		let value = syn::Ident::new("value", Span::mixed_site());
		let insert_child = insert_child(&common, &node)?;
		let insert_property = insert_property(&common, &name, &value)?;
		extra_traits.push(quote! {
			 impl #impl_gen ::knuffel::traits::DecodePartial #trait_gen
				  for #s_name #type_gen
				  #bounds
			 {
				  fn insert_child(&mut self,
						#node: &::knuffel::ast::SpannedNode<#span_ty>,
						#ctx: &mut ::knuffel::decode::Context<#span_ty>)
						-> Result<bool, ::knuffel::errors::DecodeError<#span_ty>>
				  {
						#insert_child
				  }
				  fn insert_property(&mut self,
						#name: &::knuffel::span::Spanned<Box<str>, #span_ty>,
						#value: &::knuffel::ast::Value<#span_ty>,
						#ctx: &mut ::knuffel::decode::Context<#span_ty>)
						-> Result<bool, ::knuffel::errors::DecodeError<#span_ty>>
				  {
						#insert_property
				  }
			 }
		});
	}
	if !s.has_arguments
		&& !s.has_properties
		&& s.spans.is_empty()
		&& s.node_names.is_empty()
		&& s.type_names.is_empty()
	{
		let decode_children = decode_children(&common, &children, None)?;
		extra_traits.push(quote! {
			 impl #impl_gen ::knuffel::traits::DecodeChildren #trait_gen
				  for #s_name #type_gen
				  #bounds
			 {
				  fn decode_children(
						#children: &[::knuffel::ast::SpannedNode<#span_ty>],
						#ctx: &mut ::knuffel::decode::Context<#span_ty>)
						-> Result<Self, ::knuffel::errors::DecodeError<#span_ty>>
				  {
						#decode_children
						#assign_extra
						Ok(#struct_val)
				  }
			 }
		});
	}
	Ok(quote! {
		 #(#extra_traits)*
		 impl #impl_gen ::knuffel::Decode #trait_gen for #s_name #type_gen
			  #bounds
		 {
			  fn decode_node(#node: &::knuffel::ast::SpannedNode<#span_ty>,
								  #ctx: &mut ::knuffel::decode::Context<#span_ty>)
					-> Result<Self, ::knuffel::errors::DecodeError<#span_ty>>
			  {
					#decode_specials
					#decode_args
					#decode_props
					let #children = #node.children.as_ref()
						 .map(|lst| &lst[..]).unwrap_or(&[]);
					#decode_children_normal
					#assign_extra
					Ok(#struct_val)
			  }
		 }
	})
}
pub fn emit_new_type(s: &NewType) -> syn::Result<TokenStream> {
	let s_name = &s.ident;
	let node = syn::Ident::new("node", Span::mixed_site());
	let ctx = syn::Ident::new("ctx", Span::mixed_site());
	Ok(quote! {
		 impl<S: ::knuffel::traits::ErrorSpan>
			  ::knuffel::Decode<S> for #s_name
		 {
			  fn decode_node(#node: &::knuffel::ast::SpannedNode<S>,
								  #ctx: &mut ::knuffel::decode::Context<S>)
					-> Result<Self, ::knuffel::errors::DecodeError<S>>
			  {
					if #node.arguments.len() > 0 ||
						 #node.properties.len() > 0 ||
						 #node.children.is_some()
					{
						 ::knuffel::Decode::decode_node(#node, #ctx)
							  .map(Some)
							  .map(#s_name)
					} else {
						 Ok(#s_name(None))
					}
			  }
		 }
	})
}

pub(crate) fn decode_enum_item(
	s: &Common,
	s_name: impl ToTokens,
	node: &syn::Ident,
	named: bool,
) -> syn::Result<TokenStream> {
	let children = syn::Ident::new("children", Span::mixed_site());
	let decode_args = decode_args(s, node)?;
	let decode_props = decode_props(s, node)?;
	let decode_children = decode_children(s, &children, Some(quote!(#node.span())))?;
	let assign_extra = assign_extra(s)?;
	let all_fields = s.object.all_fields();
	let struct_val = if named {
		let assignments = all_fields.iter().map(|f| f.as_assign_pair().unwrap());
		quote!(#s_name { #(#assignments,)* })
	} else {
		let mut fields = all_fields
			.iter()
			.map(|f| (f.as_index().unwrap(), &f.tmp_name))
			.collect::<Vec<_>>();
		fields.sort_by_key(|(idx, _)| *idx);
		assert_eq!(
			fields.iter().map(|(idx, _)| *idx).collect::<Vec<_>>(),
			(0..fields.len()).collect::<Vec<_>>(),
			"all tuple structure fields should be filled in"
		);
		let assignments = fields.iter().map(|(_, v)| v);
		quote! { #s_name(#(#assignments),*) }
	};
	Ok(quote! {
		 #decode_args
		 #decode_props
		 let #children = #node.children.as_ref()
			  .map(|lst| &lst[..]).unwrap_or(&[]);
		 #decode_children
		 #assign_extra
		 Ok(#struct_val)
	})
}

fn decode_value(
	val: &syn::Ident,
	ctx: &syn::Ident,
	mode: &DecodeMode,
	optional: bool,
) -> syn::Result<TokenStream> {
	match mode {
		DecodeMode::Normal => Ok(quote! {
			 ::knuffel::traits::DecodeScalar::decode(#val, #ctx)
		}),
		DecodeMode::Str if optional => {
			Ok(quote![{
				 if let Some(typ) = &#val.type_name {
					  #ctx.emit_error(::knuffel::errors::DecodeError::TypeName {
							span: typ.span().clone(),
							found: Some((**typ).clone()),
							expected: ::knuffel::errors::ExpectedType::no_type(),
							rust_type: "str", // TODO(tailhook) show field type
					  });
				 }
				 match *#val.literal {
					  ::knuffel::ast::Literal::String(ref s) => {
							::std::str::FromStr::from_str(s).map_err(|e| {
								 ::knuffel::errors::DecodeError::conversion(
									  &#val.literal, e)
							})
							.map(Some)
					  }
					  ::knuffel::ast::Literal::Null => Ok(None),
					  _ => {
							#ctx.emit_error(
								 ::knuffel::errors::DecodeError::scalar_kind(
									  ::knuffel::decode::Kind::String,
									  &#val.literal,
								 )
							);
							Ok(None)
					  }
				 }
			}])
		}
		DecodeMode::Str => {
			Ok(quote![{
				 if let Some(typ) = &#val.type_name {
					  #ctx.emit_error(::knuffel::errors::DecodeError::TypeName {
							span: typ.span().clone(),
							found: Some((**typ).clone()),
							expected: ::knuffel::errors::ExpectedType::no_type(),
							rust_type: "str", // TODO(tailhook) show field type
					  });
				 }
				 match *#val.literal {
					  ::knuffel::ast::Literal::String(ref s) => {
							::std::str::FromStr::from_str(s).map_err(|e| {
								 ::knuffel::errors::DecodeError::conversion(
									  &#val.literal, e)
							})
					  }
					  _ => Err(::knuffel::errors::DecodeError::scalar_kind(
							::knuffel::decode::Kind::String,
							&#val.literal,
					  )),
				 }
			}])
		}
		DecodeMode::Bytes if optional => Ok(quote! {
			 if matches!(&*#val.literal, ::knuffel::ast::Literal::Null) {
				  Ok(None)
			 } else {
				  match ::knuffel::decode::bytes(#val, #ctx).try_into() {
						Ok(v) => Ok(Some(v)),
						Err(e) => {
							 #ctx.emit_error(
								  ::knuffel::errors::DecodeError::conversion(
										&#val.literal, e));
							 Ok(None)
						}
				  }
			 }
		}),
		DecodeMode::Bytes => Ok(quote! {
			 ::knuffel::decode::bytes(#val, #ctx).try_into()
			 .map_err(|e| ::knuffel::errors::DecodeError::conversion(
						&#val.literal, e))
		}),
	}
}

fn decode_specials(s: &Common, node: &syn::Ident) -> syn::Result<TokenStream> {
	let ctx = s.ctx;
	let spans = s.object.spans.iter().flat_map(|span| {
		let fld = &span.field.tmp_name;
		quote! {
			 let #fld = ::knuffel::traits::DecodeSpan::decode_span(
				  #node.span(),
				  #ctx,
			 );
		}
	});
	let node_names = s.object.node_names.iter().flat_map(|node_name| {
		let fld = &node_name.field.tmp_name;
		quote! {
			 let #fld = #node.node_name.parse()
				  .map_err(|e| {
						::knuffel::errors::DecodeError::conversion(
							 &#node.node_name, e)
				  })?;
		}
	});
	let type_names = s.object.type_names.iter().flat_map(|type_name| {
		let fld = &type_name.field.tmp_name;
		if type_name.option {
			quote! {
				 let #fld = #node.type_name.as_ref().map(|tn| {
					  tn.as_str()
							.parse()
							.map_err(|e| {
								 ::knuffel::errors::DecodeError::conversion(tn, e)
							})
				 }).transpose()?;
			}
		} else {
			quote! {
				 let #fld = if let Some(tn) = #node.type_name.as_ref() {
					  tn.as_str()
							.parse()
							.map_err(|e| {
								 ::knuffel::errors::DecodeError::conversion(tn, e)
							})?
				 } else {
					  return Err(::knuffel::errors::DecodeError::missing(
							#node, "type name required"));
				 };
			}
		}
	});
	let validate_type = if s.object.type_names.is_empty() {
		Some(quote! {
			 if let Some(type_name) = &#node.type_name {
				  #ctx.emit_error(::knuffel::errors::DecodeError::unexpected(
								  type_name, "type name",
								  "no type name expected for this node"));
			 }
		})
	} else {
		None
	};
	Ok(quote! {
		 #(#spans)*
		 #(#node_names)*
		 #(#type_names)*
		 #validate_type
	})
}

fn decode_args(s: &Common, node: &syn::Ident) -> syn::Result<TokenStream> {
	let ctx = s.ctx;
	let mut decoder = Vec::new();
	let iter_args = syn::Ident::new("iter_args", Span::mixed_site());
	decoder.push(quote! {
		 let mut #iter_args = #node.arguments.iter();
	});
	for arg in &s.object.arguments {
		let fld = &arg.field.tmp_name;
		let val = syn::Ident::new("val", Span::mixed_site());
		let decode_value = decode_value(&val, ctx, &arg.decode, arg.option)?;
		match (&arg.default, &arg.kind) {
			(None, ArgKind::Value { option: true }) => {
				decoder.push(quote! {
					 let #fld = #iter_args.next().map(|#val| {
						  #decode_value
					 }).transpose()?.and_then(|v| v);
				});
			}
			(None, ArgKind::Value { option: false }) => {
				let error = if arg.field.is_indexed() {
					"additional argument is required".into()
				} else {
					format!("additional argument `{}` is required", fld.unraw())
				};
				decoder.push(quote! {
					 let #val =
						  #iter_args.next().ok_or_else(|| {
								::knuffel::errors::DecodeError::missing(
									 #node, #error)
						  })?;
					 let #fld = #decode_value?;
				});
			}
			(Some(default_value), ArgKind::Value { .. }) => {
				let default = if let Some(expr) = default_value {
					quote!(#expr)
				} else {
					quote!(::std::default::Default::default())
				};
				decoder.push(quote! {
					 let #fld = #iter_args.next().map(|#val| {
						  #decode_value
					 }).transpose()?.unwrap_or_else(|| {
						  #default
					 });
				});
			}
		}
	}
	if let Some(var_args) = &s.object.var_args {
		let fld = &var_args.field.tmp_name;
		let val = syn::Ident::new("val", Span::mixed_site());
		let decode_value = decode_value(&val, ctx, &var_args.decode, false)?;
		decoder.push(quote! {
			 let #fld = #iter_args.map(|#val| {
				  #decode_value
			 }).collect::<Result<_, _>>()?;
		});
	} else {
		decoder.push(quote! {
			 if let Some(val) = #iter_args.next() {
				  return Err(::knuffel::errors::DecodeError::unexpected(
							 &val.literal, "argument",
							 "unexpected argument"));
			 }
		});
	}
	Ok(quote! { #(#decoder)* })
}

fn decode_props(s: &Common, node: &syn::Ident) -> syn::Result<TokenStream> {
	let mut declare_empty = Vec::new();
	let mut match_branches = Vec::new();
	let mut postprocess = Vec::new();

	let ctx = s.ctx;
	let val = syn::Ident::new("val", Span::mixed_site());
	let name = syn::Ident::new("name", Span::mixed_site());
	let name_str = syn::Ident::new("name_str", Span::mixed_site());

	for prop in &s.object.properties {
		let fld = &prop.field.tmp_name;
		let prop_name = &prop.name;
		let seen_name = format_ident!("seen_{}", fld, span = Span::mixed_site());
		if prop.flatten {
			declare_empty.push(quote! {
				 let mut #fld = ::std::default::Default::default();
			});
			match_branches.push(quote! {
				 _ if ::knuffel::traits::DecodePartial::
					  insert_property(&mut #fld, #name, #val, #ctx)?
				 => {}
			});
		} else {
			let decode_value = decode_value(&val, ctx, &prop.decode, prop.option)?;
			declare_empty.push(quote! {
				 let mut #fld = None;
				 let mut #seen_name = false;
			});
			if prop.option {
				match_branches.push(quote! {
					 #prop_name => {
						  #seen_name = true;
						  #fld = #decode_value?;
					 }
				});
			} else {
				match_branches.push(quote! {
					 #prop_name => {
						  #fld = Some(#decode_value?);
					 }
				});
			}
			let req_msg = format!("property `{}` is required", prop_name);
			if let Some(value) = &prop.default {
				let default = if let Some(expr) = value {
					quote!(#expr)
				} else {
					quote!(::std::default::Default::default())
				};
				if prop.option {
					postprocess.push(quote! {
						 if !#seen_name {
							  #fld = #default;
						 };
					});
				} else {
					postprocess.push(quote! {
						 let #fld = #fld.unwrap_or_else(|| #default);
					});
				}
			} else if !prop.option {
				postprocess.push(quote! {
					 let #fld = #fld.ok_or_else(|| {
						  ::knuffel::errors::DecodeError::missing(
								#node, #req_msg)
					 })?;
				});
			}
		}
	}
	if let Some(var_props) = &s.object.var_props {
		let fld = &var_props.field.tmp_name;
		let decode_value = decode_value(&val, ctx, &var_props.decode, false)?;
		declare_empty.push(quote! {
			 let mut #fld = Vec::new();
		});
		match_branches.push(quote! {
			 #name_str => {
				  let converted_name = #name_str.parse()
						.map_err(|e| {
							 ::knuffel::errors::DecodeError::conversion(#name, e)
						})?;
				  #fld.push((
						converted_name,
						#decode_value?,
				  ));
			 }
		});
		postprocess.push(quote! {
			 let #fld = #fld.into_iter().collect();
		});
	} else {
		match_branches.push(quote! {
			 #name_str => {
				  return Err(::knuffel::errors::DecodeError::unexpected(
						#name, "property",
						format!("unexpected property `{}`",
								  #name_str.escape_default())));
			 }
		});
	};
	Ok(quote! {
		 #(#declare_empty)*
		 for (#name, #val) in #node.properties.iter() {
			  match &***#name {
					#(#match_branches)*
			  }
		 }
		 #(#postprocess)*
	})
}

fn unwrap_fn(
	parent: &Common,
	func: &syn::Ident,
	name: &syn::Ident,
	attrs: &FieldAttrs,
) -> syn::Result<TokenStream> {
	let ctx = parent.ctx;
	let span_ty = parent.span_type;
	let mut bld = StructBuilder::new(
		format_ident!("Wrap_{}", name, span = Span::mixed_site()),
		parent.object.trait_props.clone(),
		parent.object.generics.clone(),
	);
	bld.add_field(Field::new_named(name), false, false, attrs)?;
	let object = bld.build();
	let common = Common {
		object: &object,
		ctx: parent.ctx,
		span_type: parent.span_type,
	};

	let node = syn::Ident::new("node", Span::mixed_site());
	let children = syn::Ident::new("children", Span::mixed_site());
	let decode_args = decode_args(&common, &node)?;
	let decode_props = decode_props(&common, &node)?;
	let decode_children = decode_children(&common, &children, Some(quote!(#node.span())))?;
	Ok(quote! {
		 let mut #func = |#node: &::knuffel::ast::SpannedNode<#span_ty>,
								#ctx: &mut ::knuffel::decode::Context<#span_ty>|
		 {
			  #decode_args
			  #decode_props
			  let #children = #node.children.as_ref()
					.map(|lst| &lst[..]).unwrap_or(&[]);
			  #decode_children

			  Ok(#name)
		 };
	})
}

fn decode_node(
	common: &Common,
	child_def: &Child,
	in_partial: bool,
	child: &syn::Ident,
) -> syn::Result<TokenStream> {
	let ctx = common.ctx;
	let fld = &child_def.field.tmp_name;
	let dest = if in_partial {
		child_def.field.from_self()
	} else {
		quote!(#fld)
	};
	let (init, func) = if let Some(unwrap) = &child_def.unwrap {
		let func = format_ident!("unwrap_{}", fld, span = Span::mixed_site());
		let unwrap_fn = unwrap_fn(common, &func, fld, unwrap)?;
		(unwrap_fn, quote!(#func))
	} else {
		(quote!(), quote!(::knuffel::Decode::decode_node))
	};
	let value = syn::Ident::new("value", Span::mixed_site());
	let assign = if matches!(child_def.mode, ChildMode::Multi) {
		quote!(#dest.push(#value))
	} else {
		quote!(#dest = Some(#value))
	};
	if in_partial {
		Ok(quote! {
			 {
				  #init
				  let #value = #func(#child, #ctx)?;
				  #assign;
				  Ok(true)
			 }
		})
	} else {
		Ok(quote! {
			 {
				  #init
				  match #func(#child, #ctx) {
						Ok(#value) => {
							 #assign;
							 None
						}
						Err(e) => Some(Err(e)),
				  }
			 }
		})
	}
}

fn insert_child(s: &Common, node: &syn::Ident) -> syn::Result<TokenStream> {
	let ctx = s.ctx;
	let mut match_branches = Vec::with_capacity(s.object.children.len());
	for child_def in &s.object.children {
		let dest = &child_def.field.from_self();
		let child_name = &child_def.name;
		if matches!(child_def.mode, ChildMode::Flatten) {
			match_branches.push(quote! {
				 _ if ::knuffel::traits::DecodePartial
					  ::insert_child(&mut #dest, #node, #ctx)?
				 => Ok(true),
			})
		} else if matches!(child_def.mode, ChildMode::Bool) {
			let dup_err = format!(
				"duplicate node `{}`, single node expected",
				child_name.escape_default()
			);
			match_branches.push(quote! {
				 #child_name => {
					  ::knuffel::decode::check_flag_node(#node, #ctx);
					  if #dest {
							#ctx.emit_error(
								 ::knuffel::errors::DecodeError::unexpected(
									  &#node.node_name, "node", #dup_err));
					  } else {
							#dest = true;
					  }
					  Ok(true)
				 }
			});
		} else {
			let dup_err = format!(
				"duplicate node `{}`, single node expected",
				child_name.escape_default()
			);
			let decode = decode_node(s, &child_def, true, node)?;
			match_branches.push(quote! {
				 #child_name => {
					  if #dest.is_some() {
							#ctx.emit_error(
								 ::knuffel::errors::DecodeError::unexpected(
									  &#node.node_name, "node", #dup_err));
					  }
					  #decode
				 }
			});
		}
	}
	Ok(quote! {
		 match &**#node.node_name {
			  #(#match_branches)*
			  _ => Ok(false),
		 }
	})
}

fn insert_property(s: &Common, name: &syn::Ident, value: &syn::Ident) -> syn::Result<TokenStream> {
	let ctx = s.ctx;
	let mut match_branches = Vec::with_capacity(s.object.children.len());
	for prop in &s.object.properties {
		let dest = &prop.field.from_self();
		let prop_name = &prop.name;
		if prop.flatten {
			match_branches.push(quote! {
				 _ if ::knuffel::traits::DecodePartial
					  ::insert_property(&mut #dest, #name, #value, #ctx)?
				 => Ok(true),
			});
		} else {
			let decode_value = decode_value(&value, ctx, &prop.decode, prop.option)?;
			if prop.option {
				match_branches.push(quote! {
					 #prop_name => {
						  #dest = #decode_value?;
						  Ok(true)
					 }
				});
			} else {
				match_branches.push(quote! {
					 #prop_name => {
						  #dest = Some(#decode_value?);
						  Ok(true)
					 }
				});
			}
		}
	}
	Ok(quote! {
		 match &***#name {
			  #(#match_branches)*
			  _ => Ok(false),
		 }
	})
}

fn decode_children(
	s: &Common,
	children: &syn::Ident,
	err_span: Option<TokenStream>,
) -> syn::Result<TokenStream> {
	let mut declare_empty = Vec::new();
	let mut match_branches = Vec::new();
	let mut postprocess = Vec::new();

	let ctx = s.ctx;
	let child = syn::Ident::new("child", Span::mixed_site());
	let name_str = syn::Ident::new("name_str", Span::mixed_site());
	for child_def in &s.object.children {
		let fld = &child_def.field.tmp_name;
		let child_name = &child_def.name;
		match child_def.mode {
			ChildMode::Flatten => {
				declare_empty.push(quote! {
					 let mut #fld = ::std::default::Default::default();
				});
				match_branches.push(quote! {
					 _ if (
						  match ::knuffel::traits::DecodePartial
								::insert_child(&mut #fld, #child, #ctx)
						  {
								Ok(true) => return None,
								Ok(false) => false,
								Err(e) => return Some(Err(e)),
						  }
					 ) => None,
				})
			}
			ChildMode::Multi => {
				declare_empty.push(quote! {
					 let mut #fld = Vec::new();
				});
				let decode = decode_node(s, &child_def, false, &child)?;
				match_branches.push(quote! {
					 #child_name => #decode,
				});
				if let Some(default_value) = &child_def.default {
					let default = if let Some(expr) = default_value {
						quote!(#expr)
					} else {
						quote!(::std::default::Default::default())
					};
					if child_def.option {
						postprocess.push(quote! {
							 let #fld = if #fld.is_empty() {
								  #default
							 } else {
								  Some(#fld.into_iter().collect())
							 };
						});
					} else {
						postprocess.push(quote! {
							 let #fld = if #fld.is_empty() {
								  #default
							 } else {
								  #fld.into_iter().collect()
							 };
						});
					}
				} else if child_def.option {
					postprocess.push(quote! {
						 let #fld = if #fld.is_empty() {
							  None
						 } else {
							  Some(#fld.into_iter().collect())
						 };
					});
				} else {
					postprocess.push(quote! {
						 let #fld = #fld.into_iter().collect();
					});
				}
			}
			ChildMode::Normal => {
				declare_empty.push(quote! {
					 let mut #fld = None;
				});
				let dup_err = format!(
					"duplicate node `{}`, single node expected",
					child_name.escape_default()
				);
				let decode = decode_node(s, &child_def, false, &child)?;
				match_branches.push(quote! {
					 #child_name => {
						  if #fld.is_some() {
								Some(Err(
									 ::knuffel::errors::DecodeError::unexpected(
									 &#child.node_name, "node", #dup_err)))
						  } else {
								#decode
						  }
					 }
				});
				let req_msg = format!("child node `{}` is required", child_name);
				if let Some(default_value) = &child_def.default {
					let default = if let Some(expr) = default_value {
						quote!(#expr)
					} else {
						quote!(::std::default::Default::default())
					};
					postprocess.push(quote! {
						 let #fld = #fld.unwrap_or_else(|| #default);
					});
				} else if !child_def.option {
					if let Some(span) = &err_span {
						postprocess.push(quote! {
							 let #fld = #fld.ok_or_else(|| {
								  ::knuffel::errors::DecodeError::Missing {
										span: #span.clone(),
										message: #req_msg.into(),
								  }
							 })?;
						});
					} else {
						postprocess.push(quote! {
							 let #fld = #fld.ok_or_else(|| {
								  ::knuffel::errors::DecodeError::MissingNode {
										message: #req_msg.into(),
								  }
							 })?;
						});
					}
				}
			}
			ChildMode::Bool => {
				let dup_err = format!(
					"duplicate node `{}`, single node expected",
					child_name.escape_default()
				);
				declare_empty.push(quote! {
					 let mut #fld = false;
				});
				match_branches.push(quote! {
					 #child_name => {
						  ::knuffel::decode::check_flag_node(#child, #ctx);
						  if #fld {
								#ctx.emit_error(
									 ::knuffel::errors::DecodeError::unexpected(
										  &#child.node_name, "node", #dup_err));
						  } else {
								#fld = true;
						  }
						  None
					 }
				});
			}
		}
	}
	if let Some(var_children) = &s.object.var_children {
		let fld = &var_children.field.tmp_name;

		let (init, func) = if let Some(unwrap) = &var_children.unwrap {
			let func = format_ident!("unwrap_{}", fld, span = Span::mixed_site());
			let unwrap_fn = unwrap_fn(s, &func, fld, unwrap)?;
			(unwrap_fn, quote!(#func))
		} else {
			(quote!(), quote!(::knuffel::Decode::decode_node))
		};

		match_branches.push(quote! {
			 _ => {
				  #init
				  match #func(#child, #ctx) {
						Ok(#child) => Some(Ok(#child)),
						Err(e) => Some(Err(e)),
				  }
			 }
		});
		Ok(quote! {
			 #(#declare_empty)*
			 let #fld = #children.iter().flat_map(|#child| {
				  match &**#child.node_name {
						#(#match_branches)*
				  }
			 }).collect::<Result<_, ::knuffel::errors::DecodeError<_>>>()?;
			 #(#postprocess)*
		})
	} else {
		match_branches.push(quote! {
			 #name_str => {
				  #ctx.emit_error(::knuffel::errors::DecodeError::unexpected(
						#child, "node",
						format!("unexpected node `{}`",
								  #name_str.escape_default())));
				  None
			 }
		});

		Ok(quote! {
			 #(#declare_empty)*
			 #children.iter().flat_map(|#child| {
				  match &**#child.node_name {
						#(#match_branches)*
				  }
			 }).collect::<Result<(), ::knuffel::errors::DecodeError<_>>>()?;
			 #(#postprocess)*
		})
	}
}

fn assign_extra(s: &Common) -> syn::Result<TokenStream> {
	let items = s.object.extra_fields.iter().map(|fld| match fld.kind {
		ExtraKind::Auto => {
			let name = &fld.field.tmp_name;
			quote!(let #name = ::std::default::Default::default();)
		}
	});
	Ok(quote!(#(#items)*))
}
