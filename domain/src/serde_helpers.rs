//! Serde helpers shared across all domain types.

pub mod duration_secs {
    use chrono::Duration;
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    /// # Errors
    /// Propagates any error from the serializer.
    pub fn serialize<S>(d: &Duration, s: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        d.num_seconds().serialize(s)
    }

    /// # Errors
    /// Returns an error if the value cannot be deserialized as an integer.
    pub fn deserialize<'de, D>(d: D) -> Result<Duration, D::Error>
    where
        D: Deserializer<'de>,
    {
        let secs = i64::deserialize(d)?;
        Ok(Duration::seconds(secs))
    }
}
