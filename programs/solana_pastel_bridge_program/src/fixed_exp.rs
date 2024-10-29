//! Exponentiation for fixed-point numbers.
//!
//! # Usage
//!
//! ```rust
//! use fixed::types::U34F30;
//! use solana_pastel_oracle_program::fixed_exp::FixedPowF;
//!
//! let x = U34F30::from_num(4.0);
//! assert_eq!(U34F30::from_num(8.0), x.powf(U34F30::from_num(1.5)));
//! ```

use std::cmp::{Ord, Ordering};
use fixed::traits::Fixed;
use fixed::types::extra::{LeEqU128, LeEqU16, LeEqU32, LeEqU64, LeEqU8};
use fixed::{
    FixedI128, FixedI16, FixedI32, FixedI64, FixedI8, FixedU128, FixedU16, FixedU32, FixedU64,
    FixedU8,
};
use num_traits::PrimInt;
use typenum::{Bit, IsLessOrEqual, LeEq, True, U126, U127, U14, U15, U30, U31, U6, U62, U63, U7};

/// Extension trait providing fixed-point exponentiation for fixed-point numbers.
pub trait FixedPowF: Fixed {
    /// Raises a number to a fixed-point power.
    fn powf(self, n: Self) -> Self;
}

fn powi<T: Fixed>(mut x: T, mut n: i32) -> T
where
    T: Fixed + Helper,
{
    if n == 0 {
        return T::one();
    }

    let mut acc = T::one();
    while n > 0 {
        if n & 1 == 1 {
            acc *= x;
        }
        x *= x;
        n >>= 1;
    }
    acc
}

fn sqrt<T>(x: T) -> T
where
    T: Fixed + Helper,
    T::Bits: PrimInt,
{
    if x.is_zero() || x.is_one() {
        return x;
    }

    let mut pow2 = T::one();
    let mut result;

    if x < T::one() {
        while x <= pow2 * pow2 {
            pow2 >>= 1;
        }
        result = pow2;
    } else {
        while pow2 * pow2 <= x {
            pow2 <<= 1;
        }
        result = pow2 >> 1;
    }

    for _ in 0..T::NUM_BITS {
        pow2 >>= 1;
        let next_result = result + pow2;
        if next_result * next_result <= x {
            result = next_result;
        }
    }

    result
}

fn powf_01<T>(mut x: T, mut n: T) -> T
where
    T: Fixed + Helper,
    T::Bits: PrimInt + std::fmt::Debug,
{
    let mut acc = T::one();
    while !n.is_zero() {
        x = sqrt(x);
        if n.frac() >= T::half() {
            acc *= x;
            n = n.frac() - T::half();
        }
        n = n.frac() * T::two();
    }
    acc
}

fn powf<T>(x: T, n: T) -> T
where
    T: Fixed + Helper,
    T::Bits: PrimInt + std::fmt::Debug,
{
    if x.is_zero() {
        return T::ZERO;
    }

    let int = n.int();
    let frac = n.frac();

    if int.is_zero() {
        if frac.is_zero() {
            T::one()
        } else {
            powf_01(x, frac)
        }
    } else {
        let powi = powi(x, int.to_num());

        if frac.is_zero() {
            powi
        } else {
            powi * powf_01(x, frac)
        }
    }
}

macro_rules! impl_fixed_pow {
    ($fixed:ident, $le_eq:ident, $le_eq_one:ident) => {
        impl<Frac> FixedPowF for $fixed<Frac>
        where
            Frac: $le_eq + IsLessOrEqual<$le_eq_one, Output = True>,
        {
            fn powf(self, n: Self) -> Self {
                let zero = Self::from_bits(0);

                if !<LeEq<Frac, $le_eq_one>>::BOOL && n <= zero {
                    panic!(
                        "cannot raise `{}` to the power of `{}` because numbers larger than or equal to `1` are not representable",
                        self, n
                    );
                }

                match n.cmp(&zero) {
                    Ordering::Greater => powf(self, n),
                    Ordering::Equal => Self::one(),
                    Ordering::Less => powf(Self::one() / self, Helper::neg(n)),
                }
            }
        }
    };
}

impl_fixed_pow!(FixedI8, LeEqU8, U6);
impl_fixed_pow!(FixedI16, LeEqU16, U14);
impl_fixed_pow!(FixedI32, LeEqU32, U30);
impl_fixed_pow!(FixedI64, LeEqU64, U62);
impl_fixed_pow!(FixedI128, LeEqU128, U126);

impl_fixed_pow!(FixedU8, LeEqU8, U7);
impl_fixed_pow!(FixedU16, LeEqU16, U15);
impl_fixed_pow!(FixedU32, LeEqU32, U31);
impl_fixed_pow!(FixedU64, LeEqU64, U63);
impl_fixed_pow!(FixedU128, LeEqU128, U127);

trait Helper {
    const NUM_BITS: u32;
    fn is_one(self) -> bool;
    fn one() -> Self;
    fn half() -> Self;
    fn two() -> Self;
    fn neg(self) -> Self;
}

macro_rules! impl_sign_helper {
    (signed, $fixed:ident, $le_eq:ident, $le_eq_one:ident) => {
        impl<Frac: $le_eq> Helper for $fixed<Frac>
        where
            Frac: $le_eq + IsLessOrEqual<$le_eq_one>,
        {
            const NUM_BITS: u32 = <Self as Fixed>::INT_NBITS + <Self as Fixed>::FRAC_NBITS;
            fn is_one(self) -> bool {
                <LeEq<Frac, $le_eq_one>>::BOOL && self.to_bits() == 1 << Frac::U32
            }
            fn one() -> Self {
                assert!(
                    <LeEq<Frac, $le_eq_one>>::BOOL,
                    "one should be possible to represent"
                );
                Self::from_bits(1 << Frac::U32)
            }
            fn half() -> Self {
                assert!(
                    <LeEq<Frac, $le_eq_one>>::BOOL,
                    "half should be possible to represent"
                );
                Self::from_bits(1 << (Frac::U32 - 1))
            }
            fn two() -> Self {
                assert!(
                    <LeEq<Frac, $le_eq_one>>::BOOL,
                    "two should be possible to represent"
                );
                Self::from_bits(1 << (Frac::U32 + 1))
            }
            fn neg(self) -> Self {
                -self
            }
        }
    };
    (unsigned, $fixed:ident, $le_eq:ident, $le_eq_one:ident) => {
        impl<Frac: $le_eq> Helper for $fixed<Frac>
        where
            Frac: $le_eq + IsLessOrEqual<$le_eq_one>,
        {
            const NUM_BITS: u32 = <Self as Fixed>::INT_NBITS + <Self as Fixed>::FRAC_NBITS;
            fn is_one(self) -> bool {
                <LeEq<Frac, $le_eq_one>>::BOOL && self.to_bits() == 1 << Frac::U32
            }
            fn one() -> Self {
                assert!(
                    <LeEq<Frac, $le_eq_one>>::BOOL,
                    "one should be possible to represent"
                );
                Self::from_bits(1 << Frac::U32)
            }
            fn half() -> Self {
                assert!(
                    <LeEq<Frac, $le_eq_one>>::BOOL,
                    "half should be possible to represent"
                );
                Self::from_bits(1 << (Frac::U32 - 1))
            }
            fn two() -> Self {
                assert!(
                    <LeEq<Frac, $le_eq_one>>::BOOL,
                    "two should be possible to represent"
                );
                Self::from_bits(1 << (Frac::U32 + 1))
            }
            fn neg(self) -> Self {
                panic!("cannot negate an unsigned number")
            }
        }
    };
}

impl_sign_helper!(signed, FixedI8, LeEqU8, U6);
impl_sign_helper!(signed, FixedI16, LeEqU16, U14);
impl_sign_helper!(signed, FixedI32, LeEqU32, U30);
impl_sign_helper!(signed, FixedI64, LeEqU64, U62);
impl_sign_helper!(signed, FixedI128, LeEqU128, U126);

impl_sign_helper!(unsigned, FixedU8, LeEqU8, U7);
impl_sign_helper!(unsigned, FixedU16, LeEqU16, U15);
impl_sign_helper!(unsigned, FixedU32, LeEqU32, U31);
impl_sign_helper!(unsigned, FixedU64, LeEqU64, U63);
impl_sign_helper!(unsigned, FixedU128, LeEqU128, U127);

#[cfg(test)]
mod tests {
    use super::*;
    use fixed::types::U34F30;

    fn delta<T: Fixed>(a: T, b: T) -> T {
        Ord::max(a, b) - Ord::min(a, b)
    }

    fn powf_float<T: Fixed>(x: T, n: T) -> T {
        let x: f64 = x.to_num();
        let n: f64 = n.to_num();
        T::from_num(x.powf(n))
    }

    #[test]
    fn test_powf() {
        let epsilon = U34F30::from_num(0.001);

        let test_cases = &[
            (U34F30::from_num(1.0), U34F30::from_num(7.2)),
            (U34F30::from_num(0.8), U34F30::from_num(4.5)),
            (U34F30::from_num(1.2), U34F30::from_num(5.0)),
            (U34F30::from_num(2.6), U34F30::from_num(6.7)),
            (U34F30::from_num(1.6), U34F30::from_num(0.1)),
            (U34F30::from_num(5.9), U34F30::from_num(0.3)),
            (U34F30::from_num(3.1), U34F30::from_num(0.5)),
            (U34F30::from_num(9.4), U34F30::from_num(0.7)),
            (U34F30::from_num(0.7), U34F30::from_num(0.9)),
        ];

        for &(x, n) in test_cases {
            assert!(delta(powf_float(x, n), x.powf(n)) < epsilon);
        }
    }
}
