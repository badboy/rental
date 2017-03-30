#![recursion_limit = "256"]

extern crate proc_macro;
#[macro_use]
extern crate procedural_masquerade;
extern crate syn;
#[macro_use]
extern crate quote;

use std::iter;
use quote::ToTokens;


define_proc_macros! {
	#[doc(hidden)]
	#[allow(non_snake_case)]
	pub fn __rental_impl(input: &str) -> String {
		let mut tokens = quote::Tokens::new();

		for item in syn::parse_items(input).expect("Failed to parse items in module body.") {
			match item.node {
				syn::ItemKind::Use(..) => {
					item.to_tokens(&mut tokens);
				},
				syn::ItemKind::Struct(..) => {
					write_rental_struct_and_impls(&mut tokens, &item);
				},
				_ => panic!("Item must be a `use` or `struct`."),
			}
		}

		tokens.to_string()
	}
}


fn write_rental_struct_and_impls(mut tokens: &mut quote::Tokens, item: &syn::Item) {
	let (struct_data, struct_generics) = if let syn::ItemKind::Struct(ref struct_data, ref struct_generics) = item.node {
		(struct_data, struct_generics)
	} else {
		panic!("Item `{}` is not a struct.", item.ident);
	};

	let mut rattrs = item.attrs.clone();
	let mut is_rental_mut = false;
	if let Some(rental_pos) = rattrs.iter().filter(|attr| !attr.is_sugared_doc).position(|attr| match attr.value {
		syn::MetaItem::Word(ref attr_ident) => {
			is_rental_mut = match attr_ident.as_ref() {
				"rental" => false,
				"rental_mut" => true,
				_ => return false,
			};

			true
		},
		syn::MetaItem::List(ref attr_ident, ref nested) => {
			is_rental_mut = match attr_ident.as_ref() {
				"rental" => false,
				"rental_mut" => true,
				_ => return false,
			};

			if nested.get(0).is_some() {
				panic!(
					"`{}` attribute on struct `{}` must have no arguments.",
					if !is_rental_mut { "rental" } else { "rental_mut" },
					item.ident
				);
			}

			true
		},
		_ => false,
	}) {
		rattrs.remove(rental_pos);
	} else {
		panic!("Struct `{}` must have a `rental` or `rental_mut` attribute.", item.ident);
	}

	if rattrs.iter().any(|attr| !attr.is_sugared_doc) {
		panic!("Struct `{}` must not have attributes other than `rental` or `rental_mut`.", item.ident);
	}

	let fields = match *struct_data {
		syn::VariantData::Struct(ref fields) => fields,
		syn::VariantData::Tuple(..) => panic!("Struct `{}` must not be a tuple struct.", item.ident),
		_ => panic!("Struct `{}` must have at least 2 fields.", item.ident),
	};

	if fields.len() < 2 {
		panic!("Struct `{}` must have at least 2 fields.", item.ident);
	}

	let mut rfields = Vec::with_capacity(fields.len());
	for (field_idx, field) in fields.iter().enumerate() {
		if field.vis != syn::Visibility::Inherited {
			panic!(
				"Struct `{}` field `{}` must be private.",
				item.ident,
				field.ident.as_ref().map(|ident| ident.to_string()).unwrap_or_else(|| field_idx.to_string())
			);
		}

		let mut rfattrs = field.attrs.clone();
		let mut subrental = None;
		if let Some(sr_pos) = rfattrs.iter().position(|a| match a.value {
			syn::MetaItem::Word(ref ident) => {
				if ident == "subrental" {
					panic!(
						"`subrental` attribute on struct `{}` field `{}` expects `arity = int`.",
						item.ident,
						field.ident.as_ref().map(|ident| ident.to_string()).unwrap_or_else(|| field_idx.to_string())
					);
				} else { false }
			},
			syn::MetaItem::List(ref attr_ident, ref nested) => {
				if attr_ident == "subrental" {
					match (nested.get(0), nested.get(1)) {
						(
							Some(&syn::NestedMetaItem::MetaItem(syn::MetaItem::NameValue(ref sr_arity_name, syn::Lit::Int(sr_arity, ..)))),
							None
						) if sr_arity_name == "arity" => {
							if let syn::Ty::Path(ref qself, ref sr_borrow) = field.ty {
								let last_idx = sr_borrow.segments.len() - 1;

								let mut sr_borrow = sr_borrow.clone();
								sr_borrow.segments[last_idx].ident = syn::Ident::new(sr_borrow.segments[0].ident.to_string() + "_Borrow");
								sr_borrow.segments[last_idx].parameters = syn::PathParameters::none();

								let mut sr_borrow_mut = sr_borrow.clone();
								sr_borrow_mut.segments[last_idx].ident = syn::Ident::new(sr_borrow.segments[0].ident.to_string() + "_BorrowMut");
								sr_borrow_mut.segments[last_idx].parameters = syn::PathParameters::none();

								subrental = Some((sr_arity as usize, syn::Ty::Path(qself.clone(), sr_borrow), syn::Ty::Path(qself.clone(), sr_borrow_mut)));
							} else {
								panic!(
									"Struct `{}` subrental field `{}` must be a type path.",
									item.ident,
									field.ident.as_ref().map(|ident| ident.to_string()).unwrap_or_else(|| field_idx.to_string())
								);
							}
						},
						_ => panic!(
							"`subrental` attribute on struct `{}` field `{}` expects `arity = int`.",
							item.ident,
							field.ident.as_ref().map(|ident| ident.to_string()).unwrap_or_else(|| field_idx.to_string())
						),
					}

					true
				} else { false }
			},
			_ => false,
		}) {
			rfattrs.remove(sr_pos);
		}

		if rfattrs.iter().any(|attr| !attr.is_sugared_doc) {
			panic!(
				"Struct `{}` field `{}` must not have attributes other than `subrental`.",
				item.ident,
				field.ident.as_ref().map(|ident| ident.to_string()).unwrap_or_else(|| field_idx.to_string())
			);
		}

		let mut self_rlt_args = Vec::new();
		if let Some((sr_arity, ..)) = subrental {
			if let Some(ref field_ident) = field.ident {
				for sr_idx in 0 .. sr_arity {
					self_rlt_args.push(syn::Lifetime::new(format!("'{}_{}", field_ident, sr_idx)));
				}
			} else {
				for sr_idx in 0 .. sr_arity {
					self_rlt_args.push(syn::Lifetime::new(format!("'_{}_{}", field_idx, sr_idx)));
				}
			}
		} else {
			if let Some(ref field_ident) = field.ident {
				self_rlt_args.push(syn::Lifetime::new(format!("'{}", field_ident)));
			} else {
				self_rlt_args.push(syn::Lifetime::new(format!("'_{}", field_idx)));
			}
		}

		let mut used_rlt_args = Vec::new();
		let rty = {
			let mut eraser = RentalLifetimeEraser{
				fields: &rfields,
				used_rlt_args: &mut used_rlt_args,
			};

			syn::fold::noop_fold_ty(&mut eraser, field.ty.clone())
		};

		rfields.push(RentalField{
			name: if let Some(ref ident) = field.ident {
				(*ident).clone()
			} else {
				syn::Ident::new(format!("v{}", field_idx.to_string()))
			},
			orig_ty: field.ty.clone(),
			erased: syn::Field{
				ident: field.ident.clone(),
				vis: field.vis.clone(),
				attrs: rfattrs,
				ty: rty,
			},
			subrental: subrental,
			self_rlt_args: self_rlt_args,
			used_rlt_args: used_rlt_args,
		});
	}
	let fields = rfields;

	let struct_rlt_args = &fields.iter().fold(Vec::new(), |mut rlt_args, field| { rlt_args.extend(field.self_rlt_args.iter()); rlt_args });
	if let Some(collide) = struct_rlt_args.iter().find(|rlt_arg| struct_generics.lifetimes.iter().any(|lt_def| lt_def.lifetime == ***rlt_arg)) {
		panic!("Struct `{}` lifetime parameter `{}` collides with rental lifetime.", item.ident, collide.ident);
	}

	let rstruct = syn::Item{
		ident: item.ident.clone(),
		vis: item.vis.clone(),
		attrs: rattrs,
		node: syn::ItemKind::Struct(
			{
				let mut rfields_reverse: Vec<_> = fields.iter().map(|field| field.erased.clone()).collect();
				rfields_reverse.reverse();
				syn::VariantData::Struct(rfields_reverse)
			},
			struct_generics.clone()
		),
	};

	rstruct.to_tokens(tokens);

	let item_ident = &item.ident;
	let item_vis = &item.vis;

	let (struct_impl_params, struct_impl_args, struct_where_clause) = struct_generics.split_for_impl();
	let struct_lt_params = &struct_generics.lifetimes;
	let struct_ty_params = &struct_generics.ty_params;
	let struct_lt_args = &struct_lt_params.iter().map(|lt_def| &lt_def.lifetime).collect::<Vec<_>>();
	let struct_ty_args = &struct_ty_params.iter().map(|ty_param| &ty_param.ident).collect::<Vec<_>>();
	let struct_fake_rlt_args = &iter::repeat(syn::Lifetime::new("'__a")).take(struct_rlt_args.len()).collect::<Vec<_>>();

	let field_idents = &fields.iter().map(|field| &field.name).collect::<Vec<_>>();
	let local_idents = field_idents;

	let borrow_ident = syn::Ident::new(item.ident.to_string() + "_Borrow");
	let borrow_mut_ident = syn::Ident::new(item.ident.to_string() + "_BorrowMut");
	let borrow_quotes = &make_borrow_quotes(item, &fields, is_rental_mut);
	let borrow_tys = &borrow_quotes.iter().map(|&BorrowQuotes{ref ty, ..}| ty).collect::<Vec<_>>();
	let borrow_tail_ty = &borrow_tys[fields.len() - 1];
	let borrow_exprs = &borrow_quotes.iter().map(|&BorrowQuotes{ref expr, ..}| expr).collect::<Vec<_>>();
	let borrow_tail_expr = &borrow_exprs[fields.len() - 1];
	let borrow_mut_tys = &borrow_quotes.iter().map(|&BorrowQuotes{ref mut_ty, ..}| mut_ty).collect::<Vec<_>>();
	let borrow_mut_tail_ty = &borrow_mut_tys[fields.len() - 1];
	let borrow_mut_exprs = &borrow_quotes.iter().map(|&BorrowQuotes{ref mut_expr, ..}| mut_expr).collect::<Vec<_>>();
	let borrow_mut_tail_expr = &borrow_mut_exprs[fields.len() - 1];

	let borrow_lt_params = &struct_lt_params.iter().map(|lt_def| {
		let mut lt_def = (*lt_def).clone();
		lt_def.bounds.push(struct_rlt_args[0].clone());
		lt_def
	}).chain(struct_rlt_args.iter().zip(struct_rlt_args.iter().skip(1)).map(|(rlt_arg, next_rlt_arg)| {
		syn::LifetimeDef {
			attrs: Vec::with_capacity(0),
			lifetime: (**rlt_arg).clone(),
			bounds: vec![(**next_rlt_arg).clone()],
		}
	})).chain(Some(syn::LifetimeDef {
			attrs: Vec::with_capacity(0),
			lifetime: struct_rlt_args[struct_rlt_args.len() - 1].clone(),
			bounds: Vec::with_capacity(0),
	})).collect::<Vec<_>>();

	let head_local_ident = &local_idents[0];
	let head_local_ident_rep = &iter::repeat(&head_local_ident).take(fields.len() - 1).collect::<Vec<_>>();
	let head_ty = &fields[0].orig_ty;
	let prefix_field_tys = &fields.iter().map(|field| &field.erased.ty).take(fields.len() - 1).collect::<Vec<_>>();
	let suffix_local_idents = &local_idents.iter().skip(1).collect::<Vec<_>>();
	let suffix_closure_quotes = make_suffix_closure_quotes(&fields, borrow_quotes, is_rental_mut);
	let suffix_closure_bounds = &suffix_closure_quotes.iter().map(|&ClosureQuotes{ref bound, ..}| bound).collect::<Vec<_>>();
	let suffix_closure_tys = &fields.iter().skip(1).map(|field| syn::Ident::new(format!("__F{}", field.name))).collect::<Vec<_>>();
	let suffix_closure_exprs = &suffix_closure_quotes.iter().map(|&ClosureQuotes{ref expr, ..}| expr).collect::<Vec<_>>();
	let suffix_try_closure_bounds = &suffix_closure_quotes.iter().map(|&ClosureQuotes{ref try_bound, ..}| try_bound).collect::<Vec<_>>();
	let suffix_try_closure_exprs = &suffix_closure_quotes.iter().map(|&ClosureQuotes{ref try_expr, ..}| try_expr).collect::<Vec<_>>();
	let tail_rlt_args = &fields[fields.len() - 1].self_rlt_args.iter().chain(fields[fields.len() - 1].used_rlt_args.iter()).collect::<Vec<_>>();

	if !is_rental_mut {
		quote!(
			#[allow(non_camel_case_types, non_snake_case, dead_code)]
			#item_vis struct #borrow_ident<#(#borrow_lt_params,)* #(#struct_ty_params),*> #struct_where_clause {
				#(pub #local_idents: #borrow_tys,)*
			}

			#[allow(dead_code)]
			impl #struct_impl_params #item_ident #struct_impl_args #struct_where_clause {
				pub fn rent_all<__F, __R>(&self, f: __F) -> __R where
					__F: for<#(#struct_rlt_args,)*> FnOnce(#borrow_ident<#(#struct_lt_args,)* #(#struct_rlt_args,)* #(#struct_ty_args),*>) -> __R,
					__R: #(#struct_lt_args +)*,
				{
					f(unsafe { self.borrow() })
				}
			}
		).to_tokens(tokens);
	} else {
		quote!(
			#[allow(non_camel_case_types, non_snake_case, dead_code)]
			#item_vis struct #borrow_ident<#(#borrow_lt_params,)* #(#struct_ty_params),*> #struct_where_clause {
				#(#local_idents: #borrow_tys,)*
			}
		).to_tokens(tokens);
	}

	quote!(
		#[allow(non_camel_case_types, non_snake_case, dead_code)]
		#item_vis struct #borrow_mut_ident<#(#borrow_lt_params,)* #(#struct_ty_params),*> #struct_where_clause {
			#(#local_idents: #borrow_mut_tys,)*
		}

		#[allow(dead_code, unused_mut)]
		impl #struct_impl_params #item_ident #struct_impl_args #struct_where_clause {
			pub fn new<#(#suffix_closure_tys),*>(
				mut #head_local_ident: #head_ty,
				#(#suffix_local_idents: #suffix_closure_tys),*
			) -> Self where #(#suffix_closure_tys: #suffix_closure_bounds),*
			{
				#(__rental_prelude::static_assert_stable_deref::<#prefix_field_tys>();)*

				#(let mut #suffix_local_idents = unsafe { __rental_prelude::transmute(#suffix_closure_exprs) };)*

				#item_ident {
					#(#field_idents: #local_idents,)*
				}
			}

			pub fn try_new<#(#suffix_closure_tys,)* __E>(
				mut #head_local_ident: #head_ty,
				#(#suffix_local_idents: #suffix_closure_tys),*
			) -> __rental_prelude::TryNewResult<Self, __E, #head_ty> where #(#suffix_closure_tys: #suffix_try_closure_bounds),*
			{
				#(__rental_prelude::static_assert_stable_deref::<#prefix_field_tys>();)*

				#(let mut #suffix_local_idents = {
					let temp = #suffix_try_closure_exprs.map(|t| unsafe { __rental_prelude::transmute(t) });
					match temp {
						Ok(t) => t,
						Err(e) => return Err(__rental_prelude::TryNewError(e, #head_local_ident_rep)),
					}
				};)*

				Ok(#item_ident {
					#(#field_idents: #local_idents,)*
				})
			}

			pub unsafe fn borrow<'__a>(&'__a self) -> #borrow_ident<#(#struct_lt_args,)* #(#struct_fake_rlt_args,)* #(#struct_ty_args),*> {
				#borrow_ident {
					#(#local_idents: __rental_prelude::transmute(#borrow_exprs),)*
				}
			}

			pub unsafe fn borrow_mut<'__a>(&'__a mut self) -> #borrow_mut_ident<#(#struct_lt_args,)* #(#struct_fake_rlt_args,)* #(#struct_ty_args),*> {
				#borrow_mut_ident {
					#(#local_idents: __rental_prelude::transmute(#borrow_mut_exprs),)*
				}
			}

			pub fn rent<__F, __R>(&self, f: __F) -> __R where
				__F: for<#(#tail_rlt_args,)*> FnOnce(#borrow_tail_ty) -> __R,
				__R: #(#struct_lt_args +)*,
			{
				f(#borrow_tail_expr)
			}

			pub fn rent_mut<__F, __R>(&mut self, f: __F) -> __R where
				__F: for<#(#tail_rlt_args,)*> FnOnce(#borrow_mut_tail_ty) -> __R,
				__R: #(#struct_lt_args +)*,
			{
				f(#borrow_mut_tail_expr)
			}
		}
	).to_tokens(tokens);
}


fn make_borrow_quotes(item: &syn::Item, fields: &[RentalField], is_rental_mut: bool) -> Vec<BorrowQuotes> {
	(0 .. fields.len()).map(|idx| {
		if let Some(ref subrental) = fields[idx].subrental {
			let (field_lt_args, field_ty_args) = if let syn::Ty::Path(_, ref field_path) = fields[idx].orig_ty {
				let last_idx = field_path.segments.len();
				if let syn::PathParameters::AngleBracketed(ref bracket_params) = field_path.segments[last_idx].parameters {
					(&bracket_params.lifetimes, &bracket_params.types)
				} else {
					panic!("Parameters on struct `{}` subrental field `{}` must be angle-bracketed.", item.ident, fields[idx].name.to_string());
				}
			} else {
				panic!("Struct `{}` subrental field `{}` must be a type path.", item.ident, fields[idx].name.to_string());
			};

			let field_ident = &fields[idx].name;
			let self_rlt_args = &fields[idx].self_rlt_args;
			let erased_self_rlt_args = &self_rlt_args.iter().map(|_| syn::Lifetime::new("'static")).collect::<Vec<_>>();
			let sr_borrow = &subrental.1;
			let sr_borrow_mut = &subrental.2;

			BorrowQuotes {
				ty: if idx == fields.len() - 1 || !is_rental_mut {
					quote!(#sr_borrow<#(#field_lt_args,)* #(#self_rlt_args,)* #(#field_ty_args),*>)
				} else {
					quote!(__rental_prelude::PhantomData<#sr_borrow<#(#field_lt_args,)* #(#erased_self_rlt_args,)* #(#field_ty_args),*>>)
				},
				expr: if idx == fields.len() - 1 || !is_rental_mut {
					quote!(self.#field_ident.borrow())
				} else {
					quote!(__rental_prelude::PhantomData::<#sr_borrow<#(#field_ty_args),*>>)
				},
				mut_ty: if idx == fields.len() - 1 {
					quote!(#sr_borrow_mut<#(#field_lt_args,)* #(#self_rlt_args,)* #(#field_ty_args),*>)
				} else {
					quote!(__rental_prelude::PhantomData<#sr_borrow_mut<#(#field_lt_args,)* #(#erased_self_rlt_args,)* #(#field_ty_args),*>>)
				},
				mut_expr: if idx == fields.len() - 1 {
					quote!(self.#field_ident.borrow_mut())
				} else {
					quote!(__rental_prelude::PhantomData::<#sr_borrow_mut<#(#field_ty_args),*>>)
				},
				new_ty: if !is_rental_mut  {
					quote!(#sr_borrow<#(#field_lt_args,)* #(#self_rlt_args,)* #(#field_ty_args),*>)
				} else {
					quote!(#sr_borrow_mut<#(#field_lt_args,)* #(#self_rlt_args,)* #(#field_ty_args),*>)
				},
				new_expr: if !is_rental_mut {
					quote!(#field_ident.borrow())
				} else {
					quote!(#field_ident.borrow_mut())
				},
			}
		} else {
			let field_ident = &fields[idx].name;
			let field_rlt_arg = &fields[idx].self_rlt_args[0];
			let orig_ty = &fields[idx].orig_ty;
			let erased_ty = &fields[idx].erased.ty;

			BorrowQuotes {
				ty: if idx == fields.len() - 1 || !is_rental_mut {
					quote!(&#field_rlt_arg #orig_ty)
				} else {
					quote!(__rental_prelude::PhantomData<&#field_rlt_arg #erased_ty>)
				},
				expr: if idx == fields.len() - 1 || !is_rental_mut {
					quote!(&self.#field_ident)
				} else {
					quote!(__rental_prelude::PhantomData::<&#erased_ty>)
				},
				mut_ty: if idx == fields.len() - 1 {
					quote!(&#field_rlt_arg mut #orig_ty)
				} else {
					quote!(__rental_prelude::PhantomData<&#field_rlt_arg mut #erased_ty>)
				},
				mut_expr: if idx == fields.len() - 1 {
					quote!(&mut self.#field_ident)
				} else {
					quote!(__rental_prelude::PhantomData::<&mut #erased_ty>)
				},
				new_ty: if !is_rental_mut  {
					quote!(&#field_rlt_arg #orig_ty)
				} else {
					quote!(&#field_rlt_arg mut #orig_ty)
				},
				new_expr: if !is_rental_mut {
					quote!(&#field_ident)
				} else {
					quote!(&mut #field_ident)
				},
			}
		}
	}).collect()
}


fn make_suffix_closure_quotes(fields: &[RentalField], borrows: &[BorrowQuotes], is_rental_mut: bool) -> Vec<ClosureQuotes> {
	(1 .. fields.len()).map(|idx| {
		let local_name = &fields[idx].name;
		let field_ty = &fields[idx].orig_ty;

		if !is_rental_mut {
			let prev_new_tys_reverse = &borrows[0 .. idx].iter().map(|b| &b.new_ty).rev().collect::<Vec<_>>();
			let prev_new_exprs_reverse = &borrows[0 .. idx].iter().map(|b| &b.new_expr).rev().collect::<Vec<_>>();;
			let mut prev_rlt_args = Vec::new();
			for prev_field in &fields[0 .. idx] {
				prev_rlt_args.extend(&prev_field.self_rlt_args);
			}
			let prev_rlt_args = &prev_rlt_args;

			ClosureQuotes {
				bound: quote!(for<#(#prev_rlt_args),*> FnOnce(#(#prev_new_tys_reverse),*) -> #field_ty),
				expr: quote!(#local_name(#(#prev_new_exprs_reverse),*)),
				try_bound: quote!(for<#(#prev_rlt_args),*> FnOnce(#(#prev_new_tys_reverse),*) -> __rental_prelude::Result<#field_ty, __E>),
				try_expr: quote!(#local_name(#(#prev_new_exprs_reverse),*)),
			}
		} else {
			let prev_new_ty = &borrows[idx - 1].new_ty;
			let prev_new_expr = &borrows[idx - 1].new_expr;
			let prev_rlt_args = &fields[idx - 1].self_rlt_args.iter().chain(&fields[idx - 1].used_rlt_args).collect::<Vec<_>>();

			ClosureQuotes {
				bound: quote!(for<#(#prev_rlt_args),*> FnOnce(#prev_new_ty) -> #field_ty),
				expr: quote!(#local_name(#prev_new_expr)),
				try_bound: quote!(for<#(#prev_rlt_args),*> FnOnce(#prev_new_ty) -> __rental_prelude::Result<#field_ty, __E>),
				try_expr: quote!(#local_name(#prev_new_expr)),
			}
		}
	}).collect()
}


struct RentalField {
	pub name: syn::Ident,
	pub orig_ty: syn::Ty,
	pub erased: syn::Field,
	pub subrental: Option<(usize, syn::Ty, syn::Ty)>,
	pub self_rlt_args: Vec<syn::Lifetime>,
	pub used_rlt_args: Vec<syn::Lifetime>,
}


struct RentalLifetimeEraser<'a> {
	pub fields: &'a [RentalField],
	pub used_rlt_args: &'a mut Vec<syn::Lifetime>,
}


struct BorrowQuotes {
	pub ty: quote::Tokens,
	pub expr: quote::Tokens,
	pub mut_ty: quote::Tokens,
	pub mut_expr: quote::Tokens,
	pub new_ty: quote::Tokens,
	pub new_expr: quote::Tokens,
}


struct ClosureQuotes {
	pub bound: quote::Tokens,
	pub expr: quote::Tokens,
	pub try_bound: quote::Tokens,
	pub try_expr: quote::Tokens,
}


impl<'a> syn::fold::Folder for RentalLifetimeEraser<'a> {
	fn fold_lifetime(&mut self, lifetime: syn::Lifetime) -> syn::Lifetime {
		if self.fields.iter().any(|field| field.self_rlt_args.contains(&lifetime)) {
			if !self.used_rlt_args.contains(&lifetime) {
				self.used_rlt_args.push(lifetime.clone());
			}

			syn::Lifetime::new("'static")
		} else {
			lifetime
		}
	}
}
