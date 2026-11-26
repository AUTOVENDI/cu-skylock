//! A constant-velocity Kalman filter on a point in the image plane.
//!
//! The state is `[x, y, vx, vy]` in px and px/s. Between measurements the velocity takes
//! white-noise acceleration of power spectral density `q` (px²/s³) on each axis. That model
//! discretises exactly over any step `dt`, to `Q = q·[[dt³/3, dt²/2], [dt²/2, dt]]` per axis,
//! so one step of `2·dt` equals two steps of `dt` and irregular frame intervals need nothing
//! special. A measurement is the position, with variance `r` (px²) on each axis. The
//! covariance update uses the Joseph form, which keeps the covariance positive
//! semi-definite in floating point, then averages it with its transpose, which keeps it
//! symmetric.
//!
//! Times are `f64` seconds. The 4×4 maths is written out on fixed-size arrays, so nothing
//! allocates.

use bincode::{Decode, Encode};

/// A state vector: `[x, y, vx, vy]`.
pub type Vec4 = [f64; 4];
/// A 4×4 matrix, row by row.
pub type Mat4 = [[f64; 4]; 4];
/// A 2×2 matrix, row by row.
pub type Mat2 = [[f64; 2]; 2];

const IDENTITY: Mat4 = [
    [1.0, 0.0, 0.0, 0.0],
    [0.0, 1.0, 0.0, 0.0],
    [0.0, 0.0, 1.0, 0.0],
    [0.0, 0.0, 0.0, 1.0],
];

/// The state estimate and its covariance.
#[derive(Debug, Clone, Copy, Default, PartialEq, Encode, Decode)]
pub struct ConstantVelocity {
    /// `[x, y, vx, vy]`.
    pub x: Vec4,
    /// The covariance of `x`.
    pub p: Mat4,
}

/// A measured position compared with the predicted one.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Innovation {
    /// The measurement minus the predicted position.
    pub residual: [f64; 2],
    /// The inverse of the innovation covariance `S = H·P·Hᵀ + R`.
    pub s_inv: Mat2,
    /// The squared Mahalanobis distance `residualᵀ·S⁻¹·residual`.
    pub distance2: f64,
}

impl ConstantVelocity {
    /// A filter at rest at `position`, with variance `position_var` on each position axis and
    /// `velocity_var` on each velocity axis.
    pub fn new(position: [f64; 2], position_var: f64, velocity_var: f64) -> Self {
        let mut p = [[0.0; 4]; 4];
        p[0][0] = position_var;
        p[1][1] = position_var;
        p[2][2] = velocity_var;
        p[3][3] = velocity_var;
        Self {
            x: [position[0], position[1], 0.0, 0.0],
            p,
        }
    }

    /// Moves the estimate `dt` seconds ahead: `x = F·x` and `P = F·P·Fᵀ + Q`. A `dt` that
    /// isn't above 0 changes nothing.
    pub fn predict(&mut self, dt: f64, q: f64) {
        if !(dt > 0.0 && dt.is_finite()) {
            return;
        }
        self.x[0] += dt * self.x[2];
        self.x[1] += dt * self.x[3];

        // F·P: each position row gains dt times its velocity row.
        let mut p = self.p;
        for axis in 0..2 {
            let velocity_row = self.p[axis + 2];
            for (value, velocity) in p[axis].iter_mut().zip(velocity_row) {
                *value += dt * velocity;
            }
        }
        // (F·P)·Fᵀ: each position column gains dt times its velocity column.
        for row in &mut p {
            row[0] += dt * row[2];
            row[1] += dt * row[3];
        }
        let dt2 = dt * dt;
        let (q_pp, q_pv, q_vv) = (q * dt2 * dt / 3.0, q * dt2 / 2.0, q * dt);
        for axis in 0..2 {
            let (pos, vel) = (axis, axis + 2);
            p[pos][pos] += q_pp;
            p[pos][vel] += q_pv;
            p[vel][pos] += q_pv;
            p[vel][vel] += q_vv;
        }
        self.p = p;
    }

    /// Compares a measured position `z`, with variance `r` on each axis, with the predicted
    /// position. `None` if the innovation covariance isn't positive definite.
    pub fn innovation(&self, z: [f64; 2], r: f64) -> Option<Innovation> {
        let residual = [z[0] - self.x[0], z[1] - self.x[1]];
        let s = [
            [self.p[0][0] + r, self.p[0][1]],
            [self.p[1][0], self.p[1][1] + r],
        ];
        let det = s[0][0] * s[1][1] - s[0][1] * s[1][0];
        if !(det > 0.0 && det.is_finite() && s[0][0] > 0.0) {
            return None;
        }
        let s_inv = [
            [s[1][1] / det, -s[0][1] / det],
            [-s[1][0] / det, s[0][0] / det],
        ];
        let distance2 = residual[0] * (s_inv[0][0] * residual[0] + s_inv[0][1] * residual[1])
            + residual[1] * (s_inv[1][0] * residual[0] + s_inv[1][1] * residual[1]);
        distance2.is_finite().then_some(Innovation {
            residual,
            s_inv,
            distance2,
        })
    }

    /// Takes the measurement behind `innovation`, whose variance is `r` on each axis.
    pub fn update(&mut self, innovation: &Innovation, r: f64) {
        // K = P·Hᵀ·S⁻¹, where P·Hᵀ is P's first two columns.
        let s_inv = &innovation.s_inv;
        let mut k = [[0.0; 2]; 4];
        for (k_row, p_row) in k.iter_mut().zip(&self.p) {
            for (j, gain) in k_row.iter_mut().enumerate() {
                *gain = p_row[0] * s_inv[0][j] + p_row[1] * s_inv[1][j];
            }
        }
        let residual = innovation.residual;
        for (value, k_row) in self.x.iter_mut().zip(&k) {
            *value += k_row[0] * residual[0] + k_row[1] * residual[1];
        }

        // Joseph form: P = (I - K·H)·P·(I - K·H)ᵀ + K·R·Kᵀ, with R = r·I.
        let mut a = IDENTITY;
        for (a_row, k_row) in a.iter_mut().zip(&k) {
            a_row[0] -= k_row[0];
            a_row[1] -= k_row[1];
        }
        let mut p = mul_transposed(&mul(&a, &self.p), &a);
        for (p_row, k_i) in p.iter_mut().zip(&k) {
            for (value, k_j) in p_row.iter_mut().zip(&k) {
                *value += r * (k_i[0] * k_j[0] + k_i[1] * k_j[1]);
            }
        }
        // Rounding leaves the two halves a few ulps apart; average them.
        let pt = transpose(&p);
        for (row, row_t) in p.iter_mut().zip(&pt) {
            for (value, value_t) in row.iter_mut().zip(row_t) {
                *value = 0.5 * (*value + value_t);
            }
        }
        self.p = p;
    }
}

/// `a·b`.
fn mul(a: &Mat4, b: &Mat4) -> Mat4 {
    let mut out = [[0.0; 4]; 4];
    for (out_row, a_row) in out.iter_mut().zip(a) {
        for (j, value) in out_row.iter_mut().enumerate() {
            *value = a_row
                .iter()
                .zip(b)
                .map(|(a_ik, b_row)| a_ik * b_row[j])
                .sum();
        }
    }
    out
}

/// `a·bᵀ`.
fn mul_transposed(a: &Mat4, b: &Mat4) -> Mat4 {
    let mut out = [[0.0; 4]; 4];
    for (out_row, a_row) in out.iter_mut().zip(a) {
        for (value, b_row) in out_row.iter_mut().zip(b) {
            *value = a_row.iter().zip(b_row).map(|(x, y)| x * y).sum();
        }
    }
    out
}

fn transpose(m: &Mat4) -> Mat4 {
    core::array::from_fn(|i| core::array::from_fn(|j| m[j][i]))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_close(a: f64, b: f64, tolerance: f64) {
        assert!(
            (a - b).abs() <= tolerance,
            "{a} is not within {tolerance} of {b}"
        );
    }

    fn assert_mat_close(a: &Mat4, b: &Mat4, tolerance: f64) {
        for (row_a, row_b) in a.iter().zip(b) {
            for (&x, &y) in row_a.iter().zip(row_b) {
                assert_close(x, y, tolerance);
            }
        }
    }

    #[test]
    fn predict_moves_the_state_and_grows_the_covariance() {
        let mut filter = ConstantVelocity::new([10.0, 20.0], 4.0, 100.0);
        filter.x[2] = 6.0;
        filter.x[3] = -2.0;
        filter.predict(0.5, 2.0);
        assert_eq!(filter.x, [13.0, 19.0, 6.0, -2.0]);
        // Per axis: P_pp = 4 + 100·dt² + q·dt³/3, P_pv = 100·dt + q·dt²/2, P_vv = 100 + q·dt.
        let (pp, pv, vv) = (4.0 + 25.0 + 2.0 * 0.125 / 3.0, 50.0 + 0.25, 101.0);
        let expected = [
            [pp, 0.0, pv, 0.0],
            [0.0, pp, 0.0, pv],
            [pv, 0.0, vv, 0.0],
            [0.0, pv, 0.0, vv],
        ];
        assert_mat_close(&filter.p, &expected, 1e-12);
        // dt of 0 or less leaves everything alone.
        let before = filter;
        filter.predict(0.0, 2.0);
        filter.predict(-1.0, 2.0);
        assert_eq!(filter, before);
    }

    #[test]
    fn two_short_predictions_equal_one_long_one() {
        let mut once = ConstantVelocity::new([0.0, 0.0], 9.0, 400.0);
        once.x[2] = 30.0;
        once.p[0][1] = 1.5;
        once.p[1][0] = 1.5;
        let mut twice = once;
        once.predict(0.4, 5_000.0);
        twice.predict(0.15, 5_000.0);
        twice.predict(0.25, 5_000.0);
        for (a, b) in once.x.iter().zip(&twice.x) {
            assert_close(*a, *b, 1e-9);
        }
        assert_mat_close(&once.p, &twice.p, 1e-9);
    }

    #[test]
    fn the_mahalanobis_distance_matches_a_hand_computation() {
        let mut filter = ConstantVelocity::new([100.0, 50.0], 0.0, 0.0);
        filter.p[0][0] = 5.0;
        filter.p[1][1] = 12.0;
        // S = diag(5 + 4, 12 + 4), residual (3, -8): 9/9 + 64/16 = 5.
        let innovation = filter.innovation([103.0, 42.0], 4.0).unwrap();
        assert_eq!(innovation.residual, [3.0, -8.0]);
        assert_close(innovation.distance2, 5.0, 1e-12);
        // With no uncertainty at all, S is singular.
        let exact = ConstantVelocity::new([0.0, 0.0], 0.0, 0.0);
        assert!(exact.innovation([1.0, 1.0], 0.0).is_none());
    }

    #[test]
    fn an_update_pulls_halfway_when_the_variances_match() {
        let mut filter = ConstantVelocity::new([0.0, 0.0], 100.0, 50.0);
        let innovation = filter.innovation([10.0, -4.0], 100.0).unwrap();
        filter.update(&innovation, 100.0);
        // K = 100 / (100 + 100) on position; velocity has no correlation with it yet.
        assert_close(filter.x[0], 5.0, 1e-12);
        assert_close(filter.x[1], -2.0, 1e-12);
        assert_eq!((filter.x[2], filter.x[3]), (0.0, 0.0));
        assert_close(filter.p[0][0], 50.0, 1e-12);
        assert_close(filter.p[2][2], 50.0, 1e-12);
        assert_eq!(filter.p, transpose(&filter.p));
    }
}
