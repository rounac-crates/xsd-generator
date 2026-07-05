use quick_xml::{self, Reader, events::Event};
use std::{
	borrow::Cow,
	collections::{BTreeMap, HashMap},
	fs,
	num::NonZeroUsize,
	path::Path,
	str,
};

#[derive(Clone)]
pub struct Element {
	pub name: String,
	pub min: usize,
	pub max: usize,
	pub abstract_: bool,
	/// The (XML/XSD) type of this element.
	pub type_: Option<ContentType>,
	/// The named type of this element.
	pub type_name: Option<String>,
	/// Whether this element is an extension or restriction of another element.
	pub ext_rest: Option<ExtRest>,
	/// The documentation (annotations) for this element
	pub annotation: String,
	/// All attributes present in this tag. This includes any attributes used to
	/// derive other values in [Element].
	pub attrs: BTreeMap<String, String>,
	/// The values encompassed by this type. Should be empty for `ContentType::Simple`.
	pub values: Vec<Element>,
}

#[derive(Clone, Debug)]
pub enum ExtRest {
	Extend(String),
	Restrict(SimpleRestrictions),
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub enum ContentType {
	/// Simple types, with the type QName as given in the attributes.
	Simple,
	/// Complex types, with the type QName as given in the attributes.
	Complex,
	Choice,
	Schema,
}

#[derive(Clone, Debug)]
pub enum WsRestriction {
	Collapse,
	Preserve,
	Replace,
}

#[derive(Clone, Debug)]
pub enum BoundType {
	Inclusive(String),
	Exclusive(String),
}

#[derive(Clone, Debug, Default)]
pub struct SimpleRestrictions {
	pub base: String,
	pub min: Option<BoundType>,
	pub max: Option<BoundType>,
	pub total_digits: Option<NonZeroUsize>,
	pub fraction_digits: Option<usize>,
	pub length: Option<usize>,
	pub min_length: Option<usize>,
	pub max_length: Option<usize>,
	pub enumeration: Vec<Enumeration>,
	pub whitespace: Option<WsRestriction>,
	pub pattern: Option<String>,
}

#[derive(Clone, Debug, Default)]
pub struct Enumeration {
	pub value: String,
	pub annotation: String,
}

// Safety: [quick_xml] ensure UTF-8, so this is safe.
fn attr_to_str(attr: &[u8]) -> &str {
	std::str::from_utf8(attr).unwrap()
}

fn make_attr_map(attrs: HashMap<&[u8], Cow<[u8]>>) -> BTreeMap<String, String> {
	attrs
		.iter()
		.map(|(n, v)| (attr_to_str(n).to_owned(), attr_to_str(v).to_owned()))
		.collect()
}

pub fn parse_schema<P, F>(fpath: P, mut output_fn: F) -> quick_xml::Result<()>
where
	P: AsRef<Path>,
	F: FnMut(Element),
{
	let contents = fs::read(fpath)?;
	let content_str = match str::from_utf8(&contents) {
		Ok(s) => s,
		Err(e) => return Err(quick_xml::encoding::EncodingError::from(e).into()),
	};
	let mut reader = Reader::from_str(content_str);
	let mut tag_stack = Vec::new();
	let mut element_stack = Vec::new();

	loop {
		let event = reader.read_event()?;

		//eprintln!("{event:?}\nElement stack size {}", element_stack.len());
		match event {
			Event::Start(st) | Event::Empty(st) => {
				let attrs: HashMap<&[u8], Cow<[u8]>> = st
					.attributes()
					.map(|a| a.unwrap())
					.map(|a| (a.key.0, a.value))
					.collect();

				let min = match attrs.get(b"minOccurs".as_slice()) {
					Some(val) => attr_to_str(val).parse().unwrap(),
					None => 1,
				};
				let max = match attrs.get(b"maxOccurs".as_slice()) {
					Some(val) if val.as_ref() == b"unbounded" => usize::MAX,
					Some(val) => attr_to_str(val).parse().unwrap(),
					None => 1,
				};

				match st.name().0 {
					b"xs:schema" => {
						let namespace = attrs.get(b"targetNamespace".as_slice()).unwrap();

						let elem = Element {
							name: attr_to_str(namespace).to_owned(),
							min,
							max,
							abstract_: false,
							type_: Some(ContentType::Schema),
							type_name: None,
							ext_rest: None,
							annotation: String::new(),
							attrs: make_attr_map(attrs),
							values: Vec::new(),
						};

						element_stack.push(elem);
					}
					b"xs:simpleType" => {
						let name = attrs.get(b"name".as_slice()).unwrap();
						let elem = Element {
							name: attr_to_str(name).to_owned(),
							min,
							max,
							abstract_: false,
							type_: Some(ContentType::Simple),
							type_name: None,
							ext_rest: None,
							annotation: String::new(),
							attrs: make_attr_map(attrs),
							values: Vec::new(),
						};

						element_stack.push(elem);
					}
					b"xs:complexType" => {
						let name = attrs.get(b"name".as_slice()).unwrap();
						let abstract_ = match attrs.get(b"abstract".as_slice()) {
							Some(v) if *v == b"true".as_slice() => true,
							_ => false,
						};
						let elem = Element {
							name: attr_to_str(name).to_owned(),
							min,
							max,
							abstract_,
							type_: Some(ContentType::Complex),
							type_name: None,
							ext_rest: None,
							annotation: String::new(),
							attrs: make_attr_map(attrs),
							values: Vec::new(),
						};

						element_stack.push(elem);
					}
					b"xs:choice" => {
						let elem = element_stack.last_mut().unwrap();
						match elem.type_ {
							Some(ContentType::Complex) => elem.type_ = Some(ContentType::Choice),
							_ => unreachable!(),
						};
					}
					b"xs:element" => {
						let name = attrs.get(b"name".as_slice()).unwrap();
						let type_name = attrs.get(b"type".as_slice()).unwrap();

						let elem = Element {
							name: attr_to_str(name).to_owned(),
							min,
							max,
							abstract_: false,
							type_: None,
							type_name: Some(attr_to_str(type_name).to_owned()),
							ext_rest: None,
							annotation: String::new(),
							attrs: make_attr_map(attrs),
							values: Vec::new(),
						};

						element_stack.push(elem);
					}
					b"xs:enumeration" => {
						// Parse value
						let value = attrs.get(b"value".as_slice()).unwrap();
						let variant = Element {
							name: attr_to_str(value).to_owned(),
							min,
							max,
							abstract_: false,
							type_: None,
							type_name: None,
							ext_rest: None,
							annotation: String::new(),
							attrs: make_attr_map(attrs),
							values: Vec::new(),
						};

						element_stack.push(variant);
					}
					b"xs:extension" => {
						let base = attrs.get(b"base".as_slice()).unwrap();
						let elem = element_stack.last_mut().unwrap();
						elem.ext_rest = Some(ExtRest::Extend(attr_to_str(base).to_owned()));
					}
					b"xs:restriction" => {
						let base = attrs.get(b"base".as_slice()).unwrap();
						let elem = element_stack.last_mut().unwrap();
						elem.ext_rest = Some(ExtRest::Restrict(SimpleRestrictions {
							base: attr_to_str(base).to_owned(),
							..SimpleRestrictions::default()
						}));
					}
					// Restrictions
					b"xs:length" => {
						let elem = element_stack.last_mut().unwrap();
						let length_str = attrs.get(b"value".as_slice()).unwrap();
						let length = attr_to_str(length_str).parse().unwrap();

						if let Some(ExtRest::Restrict(ref mut r)) = elem.ext_rest {
							r.length = Some(length);
						}
					}
					b"xs:maxExclusive" => {
						let elem = element_stack.last_mut().unwrap();
						let value = attrs.get(b"value".as_slice()).unwrap();

						if let Some(ExtRest::Restrict(ref mut r)) = elem.ext_rest {
							r.max = Some(BoundType::Exclusive(attr_to_str(value).to_owned()));
						}
					}
					b"xs:minExclusive" => {
						let elem = element_stack.last_mut().unwrap();
						let value = attrs.get(b"value".as_slice()).unwrap();

						if let Some(ExtRest::Restrict(ref mut r)) = elem.ext_rest {
							r.min = Some(BoundType::Exclusive(attr_to_str(value).to_owned()));
						}
					}
					b"xs:maxInclusive" => {
						let elem = element_stack.last_mut().unwrap();
						let value = attrs.get(b"value".as_slice()).unwrap();

						if let Some(ExtRest::Restrict(ref mut r)) = elem.ext_rest {
							r.max = Some(BoundType::Inclusive(attr_to_str(value).to_owned()));
						}
					}
					b"xs:minInclusive" => {
						let elem = element_stack.last_mut().unwrap();
						let value = attrs.get(b"value".as_slice()).unwrap();

						if let Some(ExtRest::Restrict(ref mut r)) = elem.ext_rest {
							r.min = Some(BoundType::Inclusive(attr_to_str(value).to_owned()));
						}
					}
					b"xs:maxLength" => {
						let elem = element_stack.last_mut().unwrap();
						let val_str = attrs.get(b"value".as_slice()).unwrap();
						let val = attr_to_str(val_str).parse().unwrap();

						if let Some(ExtRest::Restrict(ref mut r)) = elem.ext_rest {
							r.max_length = Some(val);
						}
					}
					b"xs:minLength" => {
						let elem = element_stack.last_mut().unwrap();
						let val_str = attrs.get(b"value".as_slice()).unwrap();
						let val = attr_to_str(val_str).parse().unwrap();

						if let Some(ExtRest::Restrict(ref mut r)) = elem.ext_rest {
							r.min_length = Some(val);
						}
					}
					b"xs:pattern" => {
						let elem = element_stack.last_mut().unwrap();
						let pattern = attrs.get(b"value".as_slice()).unwrap();

						if let Some(ExtRest::Restrict(ref mut r)) = elem.ext_rest {
							r.pattern = Some(attr_to_str(pattern).to_owned());
						}
					}
					b"xs:whitespace" => {
						let elem = element_stack.last_mut().unwrap();
						let mode = attrs.get(b"value".as_slice()).unwrap();

						if let Some(ExtRest::Restrict(ref mut r)) = elem.ext_rest {
							let ws = match mode.as_ref() {
								b"collapse" => WsRestriction::Collapse,
								b"preserve" => WsRestriction::Preserve,
								b"replace" => WsRestriction::Replace,
								_ => panic!("Invalid Whitespace mode"),
							};
							r.whitespace = Some(ws);
						}
					}
					_ => {}
				};
				tag_stack.push(st);
			}
			Event::End(tag) => {
				// Library already verifies that document is well-formed,
				//  so no tag matching logic is needed here.
				_ = tag_stack.pop();

				match tag.name().0 {
					b"xs:simpleType" | b"xs:complexType" => {
						// Write out code for given element
						let elem = element_stack.pop().unwrap();

						_ = output_fn(elem);
					}
					b"xs:element" => {
						let elem = element_stack.pop().unwrap();

						element_stack.last_mut().unwrap().values.push(elem);
					}
					b"xs:enumeration" => {
						// Pop enumeration element
						let variant = element_stack.pop().unwrap();

						// Get element above that
						let elem = element_stack.last_mut().unwrap();
						let ExtRest::Restrict(restriction) = elem.ext_rest.as_mut().unwrap() else {
							unreachable!();
						};
						restriction.enumeration.push(Enumeration {
							value: variant.name,
							annotation: variant.annotation,
						});
					}
					b"xs:schema" => {
						let elem = element_stack.pop().unwrap();
						output_fn(elem);
					}
					_ => {}
				};
			}
			Event::Text(txt) => {
				if let Some(tag) = tag_stack.last() {
					if tag.name().0 == b"xs:documentation".as_slice() {
						let elem = element_stack.last_mut().unwrap();
						elem.annotation.push_str(attr_to_str(&txt).trim_end());
					}
				}
			}
			Event::Eof => break,
			_ => {}
		};
	}

	debug_assert_eq!(element_stack.len(), 0);

	Ok(())
}
