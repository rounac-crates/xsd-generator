//! Functions to insert serde helper functions and such.
//!

use super::Crate;
use std::{
	fmt::Write,
	path::{Path, PathBuf},
};

/// If typename has a serde mod (for `serde(with = "")`), return the mod name.
pub fn has_custom_serde(typename: &str) -> Option<&str> {
	match typename {
		"chrono::NaiveTime" => Some("crate::serde_utils::naive_time"),
		"chrono::TimeDelta" => Some("crate::serde_utils::time_delta"),
		_ => None,
	}
}

pub fn add_utils(cr8: &mut Crate) {
	// Add module declaration to root.
	let lib = cr8.contents.get_mut(Path::new("crate")).unwrap();
	lib.modules
		.insert(format!("#[macro_use]\npub mod serde_utils;"));

	// Create module file
	let m = cr8
		.contents
		.entry(PathBuf::from("crate/serde_utils"))
		.or_default();
	_ = write!(m.docs, "Module with serde helpers and utilities.");

	cr8.deps.push("iso8601 = \"0.6.3\"".to_owned());

	// Insert everything
	m.fns.push(NAIVETIME_MOD.to_owned());
	m.fns.push(TIMEDELTA_MOD.to_owned());
	m.fns.push(CHOICE_CONVERT_MACRO.to_owned());
	m.fns.push(ABSTRACT_CONVERT_MACRO.to_owned());
}

const NAIVETIME_MOD: &str = r#"
/// (De)Serializer for [chrono::NaiveTime] that add/strips the 'Z' suffix from
/// the timestamp.
pub mod naive_time {
	use chrono::NaiveTime;
	use serde::{de::Error, Deserialize, Deserializer, Serializer};

	pub fn serialize<S: Serializer>(t: &NaiveTime, ser: S) -> Result<S::Ok, S::Error> {
		let mut st = t.to_string();
		st.push('Z');
		ser.serialize_str(st.as_str())
	}
	pub fn deserialize<'de, D: Deserializer<'de>>(de: D) -> Result<NaiveTime, D::Error> {
		let st = String::deserialize(de)?;
		if let Some(s) = st.strip_suffix('Z') {
			match s.parse() {
				Err(e) => Err(D::Error::custom(e)),
				Ok(v) => Ok(v),
			}
		} else {
			Err(D::Error::custom("Expected 'Z' tz suffix"))
		}
	}
}"#;

const TIMEDELTA_MOD: &str = r#"
/// (De)Serializer for [`TimeDelta`](chrono::TimeDelta) that uses
/// [`Duration`](iso8601::Duration) to parse, then converts to/from
/// [`TimeDelta`](chrono::TimeDelta).
pub mod time_delta {
	use std::str::FromStr;
	use chrono::TimeDelta;
	use iso8601::Duration;
	use serde::{de::Error, Deserialize, Deserializer, Serializer};

	pub fn serialize<S: Serializer>(t: &TimeDelta, ser: S) -> Result<S::Ok, S::Error> {
		ser.serialize_str(&t.to_string())
	}
	pub fn deserialize<'de, D: Deserializer<'de>>(de: D) -> Result<TimeDelta, D::Error> {
		let time_string = String::deserialize(de)?;

		// From [Gregorian calendar](https://en.wikipedia.org/wiki/Gregorian_calendar#Accuracy).
		// Lower error than `365.2425` per year.
		const SECS_PER_DAY: i64 = 3600 * 24;
		const SECS_PER_HOUR: i64 = 3600;
		const SECS_PER_MIN: i64 = 60;
		const AVG_DAYS_PER_YEAR: f64 = 365.24237;
		const AVG_DAYS_PER_MONTH: f64 = AVG_DAYS_PER_YEAR / 12.0;
		const AVG_SECS_PER_YEAR: i64 = (AVG_DAYS_PER_YEAR * SECS_PER_DAY as f64) as _;
		const AVG_SECS_PER_MONTH: i64 = (AVG_DAYS_PER_MONTH * SECS_PER_DAY as f64) as _;

		// Largest `s` such that `s * 1000 + 2^32 < i64::MAX`.
		const MAX_SECS_FOR_MILLIS: i64 = 9223372032559808;

		match Duration::from_str(&time_string) {
			Ok(d) => {
				Ok(match d {
					Duration::YMDHMS { year, month, day, hour, minute, second, millisecond } => {
						let total_secs = year as i64 * AVG_SECS_PER_YEAR
							+ month as i64 * AVG_SECS_PER_MONTH
							+ day as i64 * SECS_PER_DAY
							+ hour as i64 * SECS_PER_HOUR
							+ minute as i64 * SECS_PER_MIN
							+ second as i64;

						// If there's room for
						if total_secs <= MAX_SECS_FOR_MILLIS {
							let total_msec = total_secs * 1000 + millisecond as i64;
							TimeDelta::milliseconds(total_msec)
						} else {
							TimeDelta::seconds(total_secs)
						}
					}
					Duration::Weeks(w) => TimeDelta::weeks(w as _),
				})
			}
			Err(e) => Err(D::Error::custom(e))
		}
	}
}
"#;

const CHOICE_CONVERT_MACRO: &str = r#"
/// Macro to impl [TryFrom] and [Into] for choice type enums to the serde
/// helper types.
#[macro_export]
macro_rules! choice_convert_impls {
	{
		$en_name:ident - $serde_name:ident
		$(
			$vnt_name:ident $(,)?
		)+
	} => {
		// TryFrom
		impl TryFrom<$serde_name> for $en_name {
			type Error = &'static str;
			fn try_from(value: $serde_name) -> Result<Self, Self::Error> {
				use paste::paste;

				let mut count = 0;
				let err = Err("Choice type expects one variant only.");

				let mut ret = None;
				$(
				if let Some(v) = paste![value . $vnt_name] {
					ret = Some(paste![$en_name :: $vnt_name (v)]);
					count += 1;
				}
				)+

				if count != 1 {
					err
				} else {
					// Safety: If count is > 0, then ret will have been set.
					Ok(ret.unwrap())
				}
			}
		}

		// Into
		impl Into<$serde_name> for $en_name {
			fn into(self) -> $serde_name {
				use paste::paste;
				type IntoType = $serde_name;

				match self {
					$(
						paste! [$en_name :: $vnt_name (v)] => { IntoType {
							$vnt_name: Some(v),
							..IntoType::default()
						}}
					),+
				}
			}
		}
	};
}"#;

const ABSTRACT_CONVERT_MACRO: &str = r#"
/// Macro to impl [TryFrom] and [Into] for abstract type enums to the serde
/// helper types.
#[macro_export]
macro_rules! abstract_convert_impls {
	{
		$en_name:ident - $serde_name:ident
		$(
			$vnt_name:ident $(,)?
		)+
	} => {
		// TryFrom
		impl TryFrom<$serde_name> for $en_name {
			type Error = &'static str;
			fn try_from(value: $serde_name) -> Result<Self, Self::Error> {
				use paste::paste;

				let mut count = 0;
				let err = Err("Choice type expects one variant only.");

				let mut ret = None;
				$(
				if let Some(v) = paste![value . $vnt_name] {
					ret = Some(paste![$en_name :: $vnt_name (Box::new(v))]);
					count += 1;
				}
				)+

				if count != 1 {
					err
				} else {
					// Safety: If count is > 0, then ret will have been set.
					Ok(ret.unwrap())
				}
			}
		}

		// Into
		impl Into<$serde_name> for $en_name {
			fn into(self) -> $serde_name {
				use paste::paste;
				type IntoType = $serde_name;

				match self {
					$(
						paste! [$en_name :: $vnt_name (v)] => { IntoType {
							$vnt_name: Some(*v),
							..IntoType::default()
						}}
					),+
				}
			}
		}
	};
}"#;
