//! Surface materials: the friction and restitution a contact reads.
//!
//! `docs/plan/37-materials.md` names a **surface material** as its own asset
//! on colliders, beside the render material, with friction and restitution
//! among its properties; `docs/plan/36-contact-solver.md` asks for the pair's
//! values to come from per-property combination rules — "multiply / average /
//! max" — so ice on rubber has an answer that is data rather than code.
//!
//! **The contact solver is the one consumer.** Rung 1 of
//! `36-contact-solver.md` calls [`SurfaceMaterial::combine`] on a contact's two
//! bodies each tick, and a plane carries a material of its own. The asset form,
//! the link from a render material and the other consumers `37-materials.md`
//! lists are not built.

/// How two surfaces' values for one property become the contact's.
///
/// When the two surfaces ask for different rules, the one later in this list
/// wins — [`Average`](Self::Average) gives way to anything, and
/// [`Max`](Self::Max) to nothing. That precedence is PhysX's `PxCombineMode`,
/// which Unity's physics materials expose the same way; taking it rather than
/// inventing one means a content author's expectation carries over.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub enum CombineRule {
    /// The mean of the two values.
    #[default]
    Average,
    /// The smaller value.
    Min,
    /// The product of the values.
    Multiply,
    /// The larger value.
    Max,
}

impl CombineRule {
    /// `a` and `b` combined under this rule.
    #[must_use]
    pub fn apply(self, a: f64, b: f64) -> f64 {
        match self {
            Self::Average => 0.5 * (a + b),
            Self::Min => a.min(b),
            Self::Multiply => a * b,
            Self::Max => a.max(b),
        }
    }
}

/// A surface's friction and restitution, and how each combines with the
/// surface it touches.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SurfaceMaterial {
    /// The coefficient of friction, zero or more: the ratio of the tangential
    /// force a contact can resist to the normal force pressing it.
    pub friction: f64,
    /// The coefficient of restitution, from zero (no bounce) to one (a
    /// perfectly elastic bounce): the ratio of separating to approaching speed.
    pub restitution: f64,
    /// How [`friction`](Self::friction) combines with the other surface's.
    pub friction_combine: CombineRule,
    /// How [`restitution`](Self::restitution) combines with the other
    /// surface's.
    pub restitution_combine: CombineRule,
}

impl SurfaceMaterial {
    /// Friction 0.6 and restitution 0, both averaged: Unity's default physics
    /// material, and Box2D's default friction.
    pub const DEFAULT: Self = Self::new(0.6, 0.0);

    /// A material with these coefficients and both combined by
    /// [`CombineRule::Average`].
    #[must_use]
    pub const fn new(friction: f64, restitution: f64) -> Self {
        Self {
            friction,
            restitution,
            friction_combine: CombineRule::Average,
            restitution_combine: CombineRule::Average,
        }
    }

    /// The friction and restitution a contact between `self` and `other`
    /// uses, each under the rule of higher precedence the two ask for.
    ///
    /// Symmetric: which surface is `self` does not change the answer.
    #[must_use]
    pub fn combine(&self, other: &Self) -> ContactMaterial {
        let friction_rule = self.friction_combine.max(other.friction_combine);
        let restitution_rule = self.restitution_combine.max(other.restitution_combine);
        ContactMaterial {
            friction: friction_rule.apply(self.friction, other.friction),
            restitution: restitution_rule.apply(self.restitution, other.restitution),
        }
    }
}

impl Default for SurfaceMaterial {
    fn default() -> Self {
        Self::DEFAULT
    }
}

/// The friction and restitution one contact between two surfaces uses.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ContactMaterial {
    /// The combined coefficient of friction.
    pub friction: f64,
    /// The combined coefficient of restitution.
    pub restitution: f64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_rule_combines_as_named() {
        assert_eq!(CombineRule::Average.apply(0.2, 0.6), 0.4);
        assert_eq!(CombineRule::Min.apply(0.2, 0.6), 0.2);
        assert_eq!(CombineRule::Multiply.apply(0.5, 0.6), 0.3);
        assert_eq!(CombineRule::Max.apply(0.2, 0.6), 0.6);
    }

    /// Ice on rubber: rubber asks for its grip to win and ice asks for an
    /// average, and the rule of higher precedence decides — whichever surface
    /// is asked first.
    #[test]
    fn the_higher_precedence_rule_wins_whichever_surface_asks() {
        let ice = SurfaceMaterial::new(0.05, 0.1);
        let rubber = SurfaceMaterial {
            friction_combine: CombineRule::Max,
            restitution_combine: CombineRule::Min,
            ..SurfaceMaterial::new(1.0, 0.8)
        };
        let want = ContactMaterial {
            friction: 1.0,
            restitution: 0.1,
        };
        assert_eq!(ice.combine(&rubber), want);
        assert_eq!(rubber.combine(&ice), want);

        // And with both asking for the default, the average.
        let got = ice.combine(&SurfaceMaterial::new(0.25, 0.5));
        assert_eq!(got.friction, 0.15);
        assert_eq!(got.restitution, 0.3);
    }

    #[test]
    fn the_precedence_is_average_min_multiply_max() {
        use CombineRule::{Average, Max, Min, Multiply};
        assert!(Average < Min && Min < Multiply && Multiply < Max);
    }
}
