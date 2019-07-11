mod bn128;
mod mnt;

use std::fs::File;
use zokrates_field::field::FieldPrime;

pub use self::bn128::G16;
#[cfg(feature = "libsnark")]
pub use self::bn128::GM17;
#[cfg(feature = "libsnark")]
pub use self::bn128::PGHR13;
#[cfg(feature = "libsnark")]
pub use self::mnt::PGHR13_MNT4;
#[cfg(feature = "libsnark")]
pub use self::mnt::PGHR13_MNT6;

use crate::ir;
use std::io::BufReader;

pub trait ProofSystem {
    fn setup(&self, program: ir::Prog<FieldPrime>, pk_path: &str, vk_path: &str);

    fn generate_proof(
        &self,
        program: ir::Prog<FieldPrime>,
        witness: ir::Witness<FieldPrime>,
        pk_path: &str,
        proof_path: &str,
    ) -> bool;

    fn export_solidity_verifier(&self, reader: BufReader<File>) -> String;
}
