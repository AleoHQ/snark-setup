use super::*;

impl<'a, E: PairingEngine + Sync> Phase1<'a, E> {
    /// Constructs a keypair given an RNG and a 64-byte transcript `digest`.
    pub fn key_generation<R: Rng>(rng: &mut R, digest: &[u8]) -> Result<(PublicKey<E>, PrivateKey<E>)> {
        if digest.len() != 64 {
            return Err(Error::InvalidLength {
                expected: 64,
                got: digest.len(),
            });
        }

        // tau is a contribution to the "powers of tau", in a set of points of the form "tau^i * G"
        let tau = E::Fr::rand(rng);
        // alpha and beta are a set of contributions in a form "alpha * tau^i * G" and that are required
        // for construction of the polynomials
        let alpha = E::Fr::rand(rng);
        let beta = E::Fr::rand(rng);

        let mut op = |x: E::Fr, personalization: u8| -> Result<_> {
            // Sample random g^s
            let g1_s = E::G1Projective::rand(rng).into_affine();
            // Compute g^{s*x}
            let g1_s_x = g1_s.mul(x);
            // Hash into G2 as g^{s'}
            let g2_s: E::G2Affine = compute_g2_s::<E>(&digest, &g1_s, &g1_s_x, personalization)?;
            // Compute g^{s'*x}
            let g2_s_x = g2_s.mul(x);

            Ok(((g1_s, g1_s_x), g2_s_x))
        };

        // These "public keys" are required for the next participants to check that points are in fact
        // sequential powers
        let pk_tau = op(tau, 0)?;
        let pk_alpha = op(alpha, 1)?;
        let pk_beta = op(beta, 2)?;

        Ok((
            PublicKey {
                tau_g1: pk_tau.0,
                alpha_g1: pk_alpha.0,
                beta_g1: pk_beta.0,
                tau_g2: pk_tau.1,
                alpha_g2: pk_alpha.1,
                beta_g2: pk_beta.1,
            },
            PrivateKey { tau, alpha, beta },
        ))
    }
}
