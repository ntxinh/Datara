//! Conversion from Tiberius column data to domain [`Value`].

use datara_domain::Value;
use tiberius::{ColumnData, FromSql};

/// Map one TDS cell to a domain [`Value`]. `NULL` maps to [`Value::Null`].
pub fn column_data_to_value(data: &ColumnData<'static>) -> Value {
    /// Wrap a nullable TDS value: `None` → [`Value::Null`].
    fn opt<T>(v: &Option<T>, f: impl FnOnce(&T) -> Value) -> Value {
        match v {
            Some(v) => f(v),
            None => Value::Null,
        }
    }

    /// Convert via Tiberius' `FromSql` impl for the `time` crate types and
    /// format with `Display`. A malformed wire value falls back to its debug
    /// form instead of failing the whole query.
    fn temporal<T>(d: &ColumnData<'static>) -> Value
    where
        T: for<'a> FromSql<'a> + std::fmt::Display,
    {
        match T::from_sql(d) {
            Ok(Some(t)) => Value::DateTime(t.to_string()),
            Ok(None) => Value::Null,
            Err(_) => Value::Text(format!("{d:?}")),
        }
    }

    match data {
        ColumnData::U8(v) => opt(v, |x| Value::Int(i64::from(*x))),
        ColumnData::I16(v) => opt(v, |x| Value::Int(i64::from(*x))),
        ColumnData::I32(v) => opt(v, |x| Value::Int(i64::from(*x))),
        ColumnData::I64(v) => opt(v, |x| Value::Int(*x)),
        ColumnData::Bit(v) => opt(v, |x| Value::Bool(*x)),
        ColumnData::F32(v) => opt(v, |x| Value::Float(f64::from(*x))),
        ColumnData::F64(v) => opt(v, |x| Value::Float(*x)),
        ColumnData::String(v) => opt(v, |x| Value::Text(x.to_string())),
        ColumnData::Guid(v) => opt(v, |x| Value::Uuid(*x)),
        ColumnData::Numeric(v) => opt(v, |x| Value::Decimal(x.to_string())),
        ColumnData::Binary(v) => opt(v, |x| Value::Bytes(x.to_vec())),
        ColumnData::Xml(v) => opt(v, |x| Value::Text(x.to_string())),
        ColumnData::DateTime(_) | ColumnData::SmallDateTime(_) | ColumnData::DateTime2(_) => {
            temporal::<time::PrimitiveDateTime>(data)
        }
        ColumnData::Date(_) => temporal::<time::Date>(data),
        ColumnData::Time(_) => temporal::<time::Time>(data),
        ColumnData::DateTimeOffset(_) => temporal::<time::OffsetDateTime>(data),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::borrow::Cow;

    #[test]
    fn converts_int_variants() {
        assert_eq!(
            column_data_to_value(&ColumnData::I32(Some(42))),
            Value::Int(42)
        );
        assert_eq!(
            column_data_to_value(&ColumnData::I64(Some(-7))),
            Value::Int(-7)
        );
        assert_eq!(
            column_data_to_value(&ColumnData::U8(Some(255))),
            Value::Int(255)
        );
    }

    #[test]
    fn converts_bit() {
        assert_eq!(
            column_data_to_value(&ColumnData::Bit(Some(true))),
            Value::Bool(true)
        );
        assert_eq!(
            column_data_to_value(&ColumnData::Bit(Some(false))),
            Value::Bool(false)
        );
    }

    #[test]
    fn converts_string_and_null() {
        let s = ColumnData::String(Some(Cow::Borrowed("hello")));
        assert_eq!(column_data_to_value(&s), Value::Text("hello".into()));
        assert_eq!(column_data_to_value(&ColumnData::I32(None)), Value::Null);
        assert_eq!(column_data_to_value(&ColumnData::String(None)), Value::Null);
    }

    #[test]
    fn converts_numeric_and_float() {
        let n = tiberius::numeric::Numeric::new_with_scale(12345, 2);
        assert_eq!(
            column_data_to_value(&ColumnData::Numeric(Some(n))),
            Value::Decimal("123.45".into())
        );
        assert_eq!(
            column_data_to_value(&ColumnData::F64(Some(1.5))),
            Value::Float(1.5)
        );
    }
}
