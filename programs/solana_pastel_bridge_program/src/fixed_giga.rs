use crate::big_number::CheckedMulDiv;
use fixed::types::U34F30;

pub const ZERO: u64 = 0;
pub const ONE: u64 = 1_000_000_000;
pub const TWO: u64 = 2_000_000_000;

pub const BITS_ONE: u64 = 0x40000000; // 1 << 30

pub trait FixedPow<RHS = Self> {
    /// Output type for the methods of this trait.
    type Output;

    fn pow_down(self, rhs: RHS) -> Self::Output;

    fn pow_up(self, rhs: RHS) -> Self::Output;
}

pub trait FixedExp<RHS = Self> {
    /// Output type for the methods of this trait.
    type Output;

    fn exp_down(self) -> Self::Output;

    fn exp_up(self) -> Self::Output;
}

pub trait FixedNegExp<RHS = Self> {
    /// Output type for the methods of this trait.
    type Output;

    fn neg_exp_down(self) -> Self::Output;

    fn neg_exp_up(self) -> Self::Output;
}

pub trait FixedMul<RHS = Self> {
    /// Output type for the methods of this trait.
    type Output;

    fn mul_down(self, rhs: RHS) -> Self::Output;

    fn mul_up(self, rhs: RHS) -> Self::Output;
}

pub trait FixedDiv<RHS = Self> {
    /// Output type for the methods of this trait.
    type Output;

    fn div_down(self, rhs: RHS) -> Self::Output;

    fn div_up(self, rhs: RHS) -> Self::Output;
}

pub trait FixedComplement<RHS = Self> {
    /// Output type for the methods of this trait.
    type Output;

    fn complement(self) -> Self::Output;
}

pub trait ToFixedGiga<RHS = Self> {
    /// Output type for the methods of this trait.
    type Output;

    fn to_fixed_giga(self) -> Self::Output;
}

impl FixedPow for u64 {
    type Output = u64;

    fn pow_down(self, rhs: Self) -> Self::Output {
        match rhs {
            ZERO => ONE,
            ONE => self,
            TWO => self * self,
            // _ => {
            //     let base = U34F30::from_bits(self.mul_down(BITS_ONE));
            //     let exp = U34F30::from_bits(rhs.mul_down(BITS_ONE));
            //     base.powf(exp).to_bits().div_down(BITS_ONE)
            // }
            _ => U34F30::from_num((self as f64 / 1e9).powf(rhs as f64 / 1e9))
                .to_bits()
                .div_down(BITS_ONE),
        }
    }

    fn pow_up(self, rhs: Self) -> Self::Output {
        match rhs {
            ZERO => ONE,
            ONE => self,
            TWO => self * self,
            // _ => {
            //     let base = U34F30::from_bits(self.mul_up(BITS_ONE));
            //     let exp = U34F30::from_bits(rhs.mul_up(BITS_ONE));
            //     base.powf(exp).to_bits().div_up(BITS_ONE)
            // }
            _ => U34F30::from_num((self as f64 / 1e9).powf(rhs as f64 / 1e9))
                .to_bits()
                .div_up(BITS_ONE),
        }
    }
}

impl FixedExp for u64 {
    type Output = u64;

    fn exp_down(self) -> Self::Output {
        match self {
            ZERO => ONE,
            ONE => U34F30::E.to_bits().div_down(BITS_ONE),
            // _ => U34F30::E
            //     .powf(U34F30::from_bits(self.mul_down(BITS_ONE)))
            //     .to_bits()
            //     .div_down(BITS_ONE),
            _ => U34F30::from_num((self as f64 / 1e9).exp())
                .to_bits()
                .div_down(BITS_ONE),
        }
    }

    fn exp_up(self) -> Self::Output {
        match self {
            ZERO => ONE,
            ONE => U34F30::E.to_bits().div_up(BITS_ONE),
            // _ => U34F30::E
            //     .powf(U34F30::from_bits(self.div_up(BITS_ONE)))
            //     .to_bits()
            //     .div_up(BITS_ONE),
            _ => U34F30::from_num((self as f64 / 1e9).exp())
                .to_bits()
                .div_up(BITS_ONE),
        }
    }
}

impl FixedNegExp for u64 {
    type Output = u64;

    fn neg_exp_down(self) -> Self::Output {
        match self {
            ZERO => ONE,
            _ => U34F30::from_num((-(self as f64) / 1e9).exp())
                .to_bits()
                .div_down(BITS_ONE),
        }
    }

    fn neg_exp_up(self) -> Self::Output {
        match self {
            ZERO => ONE,
            _ => U34F30::from_num((-(self as f64) / 1e9).exp())
                .to_bits()
                .div_up(BITS_ONE),
        }
    }
}

impl FixedMul for u64 {
    type Output = u64;

    fn mul_down(self, rhs: Self) -> Self::Output {
        self.checked_mul_div_down(rhs, ONE).unwrap()
    }

    fn mul_up(self, rhs: Self) -> Self::Output {
        self.checked_mul_div_up(rhs, ONE).unwrap()
    }
}

impl FixedDiv for u64 {
    type Output = u64;

    fn div_down(self, rhs: Self) -> Self::Output {
        self.checked_mul_div_down(ONE, rhs).unwrap()
    }

    fn div_up(self, rhs: Self) -> Self::Output {
        self.checked_mul_div_up(ONE, rhs).unwrap()
    }
}

impl FixedComplement for u64 {
    type Output = u64;

    fn complement(self) -> Self::Output {
        ONE.saturating_sub(self)
    }
}

impl ToFixedGiga for u64 {
    type Output = u64;

    fn to_fixed_giga(self) -> Self::Output {
        self * ONE
    }
}

impl ToFixedGiga for u32 {
    type Output = u64;

    fn to_fixed_giga(self) -> Self::Output {
        self as u64 * ONE
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_exp() {
        println!("{}", 5_000000000u64.exp_up());
        println!("{}", 5_000000000u64.exp_down());
        println!("{}", 5.0_f64.exp());
    }

    #[test]
    fn test_neg_exp() {
        println!("{}", 5_000000000u64.neg_exp_up());
        println!("{}", 5_000000000u64.neg_exp_down());
        println!("{}", (-5.0_f64).exp());
    }
}
