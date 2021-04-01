use itertools::Itertools;
use serde::{
    de::{Deserializer, Error},
    Deserialize,
    Serialize,
    Serializer,
};
use serde_diff::SerdeDiff;
use std::fmt::{self};

pub type ContributorId = String;
pub type VerifierId = String;

#[derive(Clone, Eq, PartialEq, Hash, SerdeDiff)]
pub enum Participant {
    Contributor(ContributorId),
    Verifier(VerifierId),
}

impl Participant {
    /// Creates a new contributor instance of `Participant`.
    pub fn new_contributor(participant: &str) -> Self {
        Participant::Contributor(participant.to_string())
    }

    /// Creates a new verifier instance of `Participant`.
    pub fn new_verifier(participant: &str) -> Self {
        Participant::Verifier(participant.to_string())
    }

    /// Returns `true` if the participant is a contributor.
    /// Otherwise, returns `false`.
    pub fn is_contributor(&self) -> bool {
        match self {
            Participant::Contributor(_) => true,
            Participant::Verifier(_) => false,
        }
    }

    /// Returns `true` if the participant is a verifier.
    /// Otherwise, returns `false`.
    pub fn is_verifier(&self) -> bool {
        !self.is_contributor()
    }
}

impl fmt::Display for Participant {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Participant::Contributor(contributor_id) => write!(f, "{}.contributor", contributor_id),
            Participant::Verifier(verifier_id) => write!(f, "{}.verifier", verifier_id),
        }
    }
}

impl fmt::Debug for Participant {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Participant::Contributor(contributor_id) => write!(f, "{}.contributor", contributor_id),
            Participant::Verifier(verifier_id) => write!(f, "{}.verifier", verifier_id),
        }
    }
}

impl Serialize for Participant {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Participant::Contributor(id) => serializer.serialize_str(&format!("{}.contributor", id)),
            Participant::Verifier(id) => serializer.serialize_str(&format!("{}.verifier", id)),
        }
    }
}

impl<'de> Deserialize<'de> for Participant {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Participant, D::Error> {
        let s = String::deserialize(deserializer)?;

        let (id, participant_type) = s
            .splitn(2, '.')
            .collect_tuple()
            .ok_or_else(|| D::Error::custom("unknown participant type"))?;
        let participant = match participant_type {
            "contributor" => Participant::Contributor(id.to_string()),
            "verifier" => Participant::Verifier(id.to_string()),
            _ => return Err(D::Error::custom("unknown participant type")),
        };

        Ok(participant)
    }
}
