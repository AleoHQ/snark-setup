use crate::{objects::Participant, storage::LocatorPath, CoordinatorError};

use serde::{Deserialize, Serialize};
use tracing::trace;

#[derive(Debug, Clone, Eq, PartialEq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Contribution {
    contributor_id: Option<Participant>,
    #[serde(rename = "contributedLocation")]
    contributed_locator: Option<LocatorPath>,
    #[serde(rename = "contributedSignatureLocation")]
    contributed_signature_locator: Option<LocatorPath>,
    verifier_id: Option<Participant>,
    #[serde(rename = "verifiedLocation")]
    verified_locator: Option<LocatorPath>,
    #[serde(rename = "verifiedSignatureLocation")]
    verified_signature_locator: Option<LocatorPath>,
    verified: bool,
}

impl Contribution {
    /// Returns `true` if the contribution has been verified.
    /// Otherwise returns `false`.
    #[inline]
    pub fn is_verified(&self) -> bool {
        self.verified
    }

    /// Returns a reference to the contributor, if it exists.
    /// Otherwise returns `None`.
    #[inline]
    pub fn get_contributor(&self) -> &Option<Participant> {
        &self.contributor_id
    }

    /// Returns a reference to the contributor locator, if it exists.
    /// Otherwise returns `None`.
    #[inline]
    pub fn get_contributed_location(&self) -> &Option<LocatorPath> {
        &self.contributed_locator
    }

    /// Returns a reference to the contributor signature locator, if it exists.
    /// Otherwise returns `None`.
    #[inline]
    pub fn get_contributed_signature_location(&self) -> &Option<LocatorPath> {
        &self.contributed_signature_locator
    }

    /// Returns a reference to the verifier, if it exists.
    /// Otherwise returns `None`.
    #[allow(dead_code)]
    #[inline]
    pub fn get_verifier(&self) -> &Option<Participant> {
        &self.verifier_id
    }

    /// Returns a reference to the verifier locator, if it exists.
    /// Otherwise returns `None`.
    #[inline]
    pub fn get_verified_location(&self) -> &Option<LocatorPath> {
        &self.verified_locator
    }

    /// Returns a reference to the verifier signature locator, if it exists.
    /// Otherwise returns `None`.
    #[inline]
    pub fn get_verified_signature_location(&self) -> &Option<LocatorPath> {
        &self.verified_signature_locator
    }

    ///
    /// Creates a new contributor instance of `Contribution`.
    ///
    /// This function is called when adding a new contribution
    /// to an *existing* chunk only. It should be assumed that
    /// the chunk is in the process of being locked by a new
    /// contributor when calling this function.
    ///
    #[inline]
    pub(crate) fn new_contributor(
        participant: Participant,
        contributed_locator: LocatorPath,
        contributed_signature_locator: LocatorPath,
    ) -> Result<Self, CoordinatorError> {
        // Check that the participant is a contributor.
        if !participant.is_contributor() {
            return Err(CoordinatorError::ExpectedContributor);
        }

        Ok(Self {
            contributor_id: Some(participant),
            contributed_locator: Some(contributed_locator),
            contributed_signature_locator: Some(contributed_signature_locator),
            verifier_id: None,
            verified_locator: None,
            verified_signature_locator: None,
            verified: false,
        })
    }

    ///
    /// Creates a new verifier instance of `Contribution`.
    ///
    /// This function is called when adding a new contribution
    /// to a *new* chunk only. It should be assumed that
    /// the chunk is new and in the process of being created
    /// by the coordinator when calling this function.
    ///
    #[inline]
    pub(crate) fn new_verifier(
        contribution_id: u64,
        participant: Participant,
        verified_locator: LocatorPath,
        verified_signature_locator: LocatorPath,
    ) -> Result<Self, CoordinatorError> {
        // Check that the participant is a verifier.
        if !participant.is_verifier() {
            return Err(CoordinatorError::ExpectedVerifier);
        }

        // Check that this function is only used to
        // initialize a new round in the ceremony.
        if contribution_id != 0 {
            return Err(CoordinatorError::ContributionIdIsNonzero);
        }

        // Create a new contribution instance.
        // As this is function is only used for initialization,
        // we can safely set `verified` to `true`.
        let contribution = Self {
            contributor_id: None,
            contributed_locator: None,
            contributed_signature_locator: None,
            verifier_id: Some(participant),
            verified_locator: Some(verified_locator),
            verified_signature_locator: Some(verified_signature_locator),
            verified: true,
        };

        Ok(contribution)
    }

    ///
    /// Assign a verifier to this instance of `Contribution`.
    ///
    /// If this contribution already has a verifier or verified locator,
    /// returns a `CoordinatorError`.
    ///
    /// Note that this function does NOT set the state of `verified`.
    /// This approach allows a verifier to assign themselves to verify
    /// this contribution, and only after completing verification,
    /// proceed to call another function to update `verified`.
    ///
    #[inline]
    pub(crate) fn assign_verifier(
        &mut self,
        participant: Participant,
        verified_locator: LocatorPath,
        verified_signature_locator: LocatorPath,
    ) -> Result<(), CoordinatorError> {
        // Check that the participant is a verifier.
        if !participant.is_verifier() {
            return Err(CoordinatorError::ExpectedVerifier);
        }

        // Check that this contribution does not have a verifier.
        if self.verifier_id.is_some() {
            return Err(CoordinatorError::ContributionAlreadyAssignedVerifier);
        }

        // Check that this contribution does not have a verified locator.
        if self.verified_locator.is_some() {
            return Err(CoordinatorError::ContributionAlreadyAssignedVerifiedLocator);
        }

        // Check that this contribution is not verified.
        if self.verified {
            return Err(CoordinatorError::ContributionAlreadyVerified);
        }

        self.verifier_id = Some(participant);
        self.verified_locator = Some(verified_locator);
        self.verified_signature_locator = Some(verified_signature_locator);
        Ok(())
    }

    /// Updates `verified` to `true` in this instance of `Contribution`,
    /// if the verifier ID and verified location are valid.
    ///
    /// If this contribution has already been verified,
    /// returns a `CoordinatorError`.
    ///
    #[tracing::instrument(
        level = "error",
        skip(self, participant),
        fields(participant = %participant),
        err
    )]
    pub(crate) fn set_verified(&mut self, participant: &Participant) -> Result<(), CoordinatorError> {
        // Check that the participant is a verifier.
        if !participant.is_verifier() {
            return Err(CoordinatorError::ExpectedVerifier);
        }

        // Check that this contribution has a verifier.
        let verifier_id = match &self.verifier_id {
            Some(verifier_id) => verifier_id,
            None => return Err(CoordinatorError::ContributionMissingVerifier),
        };

        // Check that the verifier matches the given participant.
        if *verifier_id != *participant {
            return Err(CoordinatorError::UnauthorizedChunkVerifier);
        }

        // Check that this contribution has a verified locator.
        if self.verified_locator.is_none() {
            return Err(CoordinatorError::ContributionMissingVerifiedLocator);
        }

        // Check that this contribution has not been verified.
        if self.verified {
            return Err(CoordinatorError::ContributionAlreadyVerified);
        }

        trace!("Setting contribution to verified");
        self.verified = true;
        Ok(())
    }

    /// Get a list containing all the file locators associated with
    /// this contribution.
    pub(crate) fn get_locators(&self) -> Vec<LocatorPath> {
        let mut paths: Vec<LocatorPath> = Vec::new();

        if let Some(path) = self.get_contributed_location().clone() {
            paths.push(path)
        }

        if let Some(path) = self.get_contributed_signature_location().clone() {
            paths.push(path)
        }

        if let Some(path) = self.get_verified_location().clone() {
            paths.push(path)
        }

        if let Some(path) = self.get_verified_signature_location().clone() {
            paths.push(path)
        }

        paths
    }
}
