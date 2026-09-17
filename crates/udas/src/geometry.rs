//! Geometry Layer — pure math, zero LLM token cost.
//!
//! Responsible for all geometric operations on the emergent disk coordinate
//! system: MDS projection, angle bisection, coordinate maintenance, and
//! out-of-sample projection.
//!
//! ## Key Principles
//!
//! - **No predefined axes** — the coordinate system emerges from the
//!   embedding distribution via MDS, not from any preset frame.
//! - **Information distance**: d(A,B) = 1 - cos_sim(emb_A, emb_B)
//! - **Cold start**: 3 maximally-dissimilar restorations uniquely define
//!   a 2D coordinate system (3 non-collinear points).

use crate::types::{Angle, DiskPoint, Embedding};

// ─── Classical MDS Core ──────────────────────────────────────────────────

/// Compute the cosine-distance matrix from a set of embedding vectors.
///
/// D[i][j] = 1 - cos_sim(emb[i], emb[j])
pub fn cosine_distance_matrix(embeddings: &[Embedding]) -> anyhow::Result<Vec<Vec<f64>>> {
    let n = embeddings.len();
    let mut matrix = vec![vec![0.0; n]; n];
    for i in 0..n {
        for j in 0..n {
            if i == j { continue; }
            let dot: f64 = embeddings[i].iter().zip(&embeddings[j]).map(|(a, b)| a * b).sum();
            let norm_i = embeddings[i].iter().map(|v: &f64| v.powi(2)).sum::<f64>().sqrt();
            let norm_j = embeddings[j].iter().map(|v: &f64| v.powi(2)).sum::<f64>().sqrt();
            matrix[i][j] = 1.0 - dot / (norm_i * norm_j).max(f64::EPSILON);
        }
    }
    Ok(matrix)
}

/// Classical MDS: given a distance matrix, compute 2D coordinates.
///
/// Algorithm:
/// 1. Square the distances -> D^2
/// 2. Double-center: B = -1/2 J D^2 J  where J = I - (1/n) 11^T
/// 3. Eigendecompose B, take top-2 eigenvalues/eigenvectors
/// 4. X = V_2 Lambda_2^(1/2)
///
/// For n=3 (cold start), this has a unique solution since 3 non-collinear
/// points fully determine a 2D coordinate system.
fn classical_mds_2d(distances: &[Vec<f64>]) -> anyhow::Result<Vec<DiskPoint>> {
    let n = distances.len();
    if n < 2 {
        anyhow::bail!("MDS requires at least 2 points, got {}", n);
    }

    // Step 1: Square the distance matrix
    let d_sq: Vec<Vec<f64>> = distances
        .iter()
        .map(|row| row.iter().map(|&d| d * d).collect())
        .collect();

    // Step 2: Double centering -- B = -1/2 J D^2 J
    // J = I - (1/n) * 1*1^T
    // B[i][j] = -1/2 (D^2[i][j] - mean_i(D^2[i][j]) - mean_j(D^2[i][j]) + mean(D^2))
    let mut row_means = vec![0.0; n];
    let mut col_means = vec![0.0; n];
    let mut grand_mean = 0.0;
    for i in 0..n {
        for j in 0..n {
            row_means[i] += d_sq[i][j];
            col_means[j] += d_sq[i][j];
            grand_mean += d_sq[i][j];
        }
    }
    for i in 0..n { row_means[i] /= n as f64; }
    for j in 0..n { col_means[j] /= n as f64; }
    grand_mean /= (n * n) as f64;

    let mut b = vec![vec![0.0; n]; n];
    for i in 0..n {
        for j in 0..n {
            b[i][j] = -0.5 * (d_sq[i][j] - row_means[i] - col_means[j] + grand_mean);
        }
    }

    // Step 3: Eigendecomposition via Jacobi method (for small symmetric matrices)
    let (eigenvalues, eigenvectors) = jacobi_eigen(&b)?;

    // Step 4: Find top-2 eigenvalues and their eigenvectors
    // Sort indices by eigenvalue (descending)
    let mut indices: Vec<usize> = (0..n).collect();
    indices.sort_by(|&a, &b| eigenvalues[b].partial_cmp(&eigenvalues[a]).unwrap());

    // Take top 2 (pad with zeros if n < 2, though we already checked n >= 2)
    let mut coords = vec![DiskPoint { x: 0.0, y: 0.0 }; n];
    for dim in 0..2.min(n) {
        let idx = indices[dim];
        let lambda = eigenvalues[idx].max(0.0).sqrt(); // clamp negative eigenvalues
        for i in 0..n {
            if dim == 0 {
                coords[i].x = eigenvectors[idx][i] * lambda;
            } else {
                coords[i].y = eigenvectors[idx][i] * lambda;
            }
        }
    }

    Ok(coords)
}

/// Jacobi eigenvalue algorithm for symmetric matrices.
///
/// Returns (eigenvalues, eigenvectors) where eigenvectors[k] is the k-th eigenvector.
/// Suitable for small matrices (n <= 50). O(n^3) per sweep, typically 5-10 sweeps.
fn jacobi_eigen(matrix: &[Vec<f64>]) -> anyhow::Result<(Vec<f64>, Vec<Vec<f64>>)> {
    let n = matrix.len();
    if n == 0 { anyhow::bail!("empty matrix"); }

    let mut a: Vec<Vec<f64>> = matrix.iter().map(|r| r.to_vec()).collect();
    let mut v: Vec<Vec<f64>> = (0..n).map(|i| {
        (0..n).map(|j| if i == j { 1.0 } else { 0.0 }).collect()
    }).collect();

    let max_sweeps = 100;
    let tolerance = 1e-12;

    for _ in 0..max_sweeps {
        // Compute off-diagonal sum
        let mut off_diag: f64 = 0.0;
        for i in 0..n {
            for j in (i + 1)..n {
                off_diag += a[i][j].abs();
            }
        }

        if off_diag < tolerance {
            break;
        }

        for p in 0..n {
            for q in (p + 1)..n {
                let apq = a[p][q];
                if apq.abs() < tolerance { continue; }

                let app = a[p][p];
                let aqq = a[q][q];
                let theta = (aqq - app) / (2.0 * apq);
                let t = theta.signum() / (theta.abs() + (1.0 + theta * theta).sqrt());
                let c = 1.0 / (1.0 + t * t).sqrt();
                let s = t * c;

                // Apply rotation
                a[p][p] = app - t * apq;
                a[q][q] = aqq + t * apq;
                a[p][q] = 0.0;
                a[q][p] = 0.0;

                for i in 0..n {
                    if i != p && i != q {
                        let aip = a[i][p];
                        let aiq = a[i][q];
                        a[i][p] = c * aip - s * aiq;
                        a[p][i] = a[i][p];
                        a[i][q] = s * aip + c * aiq;
                        a[q][i] = a[i][q];
                    }
                    let vip = v[i][p];
                    let viq = v[i][q];
                    v[i][p] = c * vip - s * viq;
                    v[i][q] = s * vip + c * viq;
                }
            }
        }
    }

    // Extract eigenvalues (diagonal) and eigenvectors (columns of v)
    let eigenvalues: Vec<f64> = (0..n).map(|i| a[i][i]).collect();
    // eigenvectors[k] = column k of v = [v[0][k], v[1][k], ..., v[n-1][k]]
    let eigenvectors: Vec<Vec<f64>> = (0..n).map(|k| {
        (0..n).map(|i| v[i][k]).collect()
    }).collect();

    Ok((eigenvalues, eigenvectors))
}

// ─── Public API ──────────────────────────────────────────────────────────

/// Cold-start: given 3 initial restorations (fact-chain, intuition,
/// counter-factual), compute embedding vectors -> cosine distance matrix ->
/// classical MDS -> 2D disk coordinates.
///
/// Returns the 3 disk points that define the initial coordinate system.
pub fn cold_start_mds(embeddings: &[Embedding; 3]) -> anyhow::Result<[DiskPoint; 3]> {
    let dists = cosine_distance_matrix(embeddings)?;
    let coords = classical_mds_2d(&dists)?;
    if coords.len() != 3 {
        anyhow::bail!("MDS produced {} points, expected 3", coords.len());
    }
    Ok([coords[0], coords[1], coords[2]])
}

/// Project a new embedding onto the existing 2D disk via out-of-sample MDS.
///
/// Uses the distance-based triangulation method:
/// 1. Compute distances from new point to all existing points
/// 2. Minimize stress in 2D: find (x,y) that best preserves distances
/// 3. Solved analytically via least-squares on the distance equations
///
/// Complexity: O(N*d) for distance computation + O(N) for projection.
/// Sub-millisecond for typical problem sizes.
pub fn out_of_sample_project(
    embedding: &Embedding,
    existing_points: &[DiskPoint],
    existing_embeddings: &[Embedding],
) -> anyhow::Result<DiskPoint> {
    let n = existing_points.len();
    if n == 0 {
        anyhow::bail!("need at least 1 existing point for projection");
    }
    if n != existing_embeddings.len() {
        anyhow::bail!("mismatch: {} points vs {} embeddings", n, existing_embeddings.len());
    }

    // Compute cosine distances from new embedding to all existing
    let new_norm: f64 = embedding.iter().map(|v| v.powi(2)).sum::<f64>().sqrt();
    let mut dists = vec![0.0; n];
    for (i, emb) in existing_embeddings.iter().enumerate() {
        let dot: f64 = embedding.iter().zip(emb).map(|(a, b)| a * b).sum();
        let emb_norm: f64 = emb.iter().map(|v| v.powi(2)).sum::<f64>().sqrt();
        dists[i] = 1.0 - dot / (new_norm * emb_norm).max(f64::EPSILON);
    }

    if n == 1 {
        // Single reference: place at distance d along x-axis from existing point
        let p = existing_points[0];
        return Ok(DiskPoint { x: p.x + dists[0], y: p.y });
    }

    if n == 2 {
        // Two references: intersect two circles in 2D
        let p0 = existing_points[0];
        let p1 = existing_points[1];
        let d0 = dists[0];
        let d1 = dists[1];

        let dx = p1.x - p0.x;
        let dy = p1.y - p0.y;
        let d01 = (dx * dx + dy * dy).sqrt();
        if d01 < f64::EPSILON {
            // Coincident points -- fallback
            return Ok(DiskPoint {
                x: p0.x + d0 * 0.5,
                y: p0.y + d0 * 0.5,
            });
        }

        // Law of cosines: a = (d0^2 - d1^2 + d01^2) / (2*d01)
        let a = (d0 * d0 - d1 * d1 + d01 * d01) / (2.0 * d01);
        let h_sq = d0 * d0 - a * a;
        let h = if h_sq > 0.0 { h_sq.sqrt() } else { 0.0 };

        // Point along the line p0->p1 at distance a, then perpendicular h
        let ux = dx / d01;
        let uy = dy / d01;
        // Choose positive perpendicular (arbitrary -- sign doesn't matter for disk)
        return Ok(DiskPoint {
            x: p0.x + a * ux - h * uy,
            y: p0.y + a * uy + h * ux,
        })
    }

    // n >= 3: least-squares trilateration
    // Build system: for each reference i, |x - p_i|^2 ~= d_i^2
    // Expanding: -2*p_i.x + |x|^2 ~= d_i^2 - |p_i|^2
    // Subtract equation 0 from equation i to eliminate |x|^2:
    //   2*(p_i - p_0).x ~= d_0^2 - d_i^2 + |p_i|^2 - |p_0|^2
    // This gives (n-1) linear equations in 2 unknowns -> least squares.

    let p0 = existing_points[0];
    let d0_sq = dists[0] * dists[0];

    let mut ata = [[0.0f64; 2]; 2]; // 2x2 normal equations matrix
    let mut atb = [0.0f64; 2];      // 2x1 RHS

    for i in 1..n {
        let pi = existing_points[i];
        let di_sq = dists[i] * dists[i];

        // Coefficients: 2*(pi.x - p0.x), 2*(pi.y - p0.y)
        let row_x = 2.0 * (pi.x - p0.x);
        let row_y = 2.0 * (pi.y - p0.y);
        // RHS: d0^2 - di^2 + |pi|^2 - |p0|^2
        let rhs = d0_sq - di_sq
            + (pi.x * pi.x + pi.y * pi.y)
            - (p0.x * p0.x + p0.y * p0.y);

        ata[0][0] += row_x * row_x;
        ata[0][1] += row_x * row_y;
        ata[1][0] += row_y * row_x;
        ata[1][1] += row_y * row_y;
        atb[0] += row_x * rhs;
        atb[1] += row_y * rhs;
    }

    // Solve 2x2 system: ata * x = atb
    let det = ata[0][0] * ata[1][1] - ata[0][1] * ata[1][0];
    if det.abs() < f64::EPSILON {
        // Singular -- fallback to simple average
        let mut x = 0.0;
        let mut y = 0.0;
        for p in existing_points {
            x += p.x;
            y += p.y;
        }
        return Ok(DiskPoint { x: x / n as f64, y: y / n as f64 });
    }

    let x = (ata[1][1] * atb[0] - ata[0][1] * atb[1]) / det;
    let y = (ata[0][0] * atb[1] - ata[1][0] * atb[0]) / det;

    Ok(DiskPoint { x, y })
}

/// Incrementally refine the MDS coordinate system.
///
/// Called every 3 new restorations. Re-projects all visited points using
/// full classical MDS on the complete embedding set.
///
/// This is the "global refresh" strategy -- more accurate than incremental
/// updates, at O(N^3) cost (acceptable for N < 100).
pub fn incremental_mds_refinement(
    points: &[DiskPoint],
    embeddings: &[Embedding],
) -> anyhow::Result<Vec<DiskPoint>> {
    if embeddings.is_empty() {
        return Ok(Vec::new());
    }
    if points.len() != embeddings.len() {
        anyhow::bail!(
            "mismatch: {} points vs {} embeddings",
            points.len(),
            embeddings.len()
        );
    }

    // Recompute distance matrix from all embeddings
    let dists = cosine_distance_matrix(embeddings)?;

    // Run full classical MDS on the complete set
    let new_coords = classical_mds_2d(&dists)?;

    // Preserve angular orientation: if the new coordinate system is rotated
    // or reflected relative to the old one, align them via Procrustes.
    procrustes_align(&new_coords, points)
}



// ─── MDS Phase Computation (for Complex Framework) ─────────────────────
//
// The complex framework needs phase angles φₖ for each measurement.
// These phases come from MDS projection: the 2D coordinates (xₖ, yₖ)
// give a natural angular phase via atan2(yₖ, xₖ).
//
// This is "geometric phase derivation" (方案A from the long-range plan):
// the phase is not from quantum hardware but from the embedding's
// position in the similarity space. Different embedding clusters
// naturally get different phases, enabling complex interference.

/// Compute MDS-derived phases for a set of embeddings.
///
/// Given N embedding vectors, compute their 2D MDS coordinates and
/// extract the angular phase φₖ = atan2(yₖ, xₖ) for each.
///
/// This is the "geometric phase" source for the complex framework:
/// the phase comes from the embedding's position in the similarity
/// space, not from any external quantum state.
///
/// Returns a vector of phase angles in radians [-π, π].
pub fn compute_mds_phases(embeddings: &[Embedding]) -> anyhow::Result<Vec<f64>> {
    if embeddings.is_empty() {
        return Ok(Vec::new());
    }
    if embeddings.len() == 1 {
        return Ok(vec![0.0]);
    }
    let dists = cosine_distance_matrix(embeddings)?;
    let coords = classical_mds_2d(&dists)?;
    Ok(coords.iter().map(|p| p.y.atan2(p.x)).collect())
}

/// Compute MDS coordinates for a set of embeddings (public wrapper).
///
/// Exposes the classical MDS computation for external use (e.g.,
/// visualization, phase computation, coordinate system initialization).
pub fn compute_mds_coordinates(embeddings: &[Embedding]) -> anyhow::Result<Vec<DiskPoint>> {
    if embeddings.is_empty() {
        return Ok(Vec::new());
    }
    if embeddings.len() == 1 {
        return Ok(vec![DiskPoint { x: 0.0, y: 0.0 }]);
    }
    let dists = cosine_distance_matrix(embeddings)?;
    classical_mds_2d(&dists)
}

/// Compute the MDS phase for a single new embedding relative to existing ones.
///
/// Projects the new embedding onto the existing MDS disk via
/// out-of-sample projection and returns its angular phase.
pub fn compute_single_phase(
    embedding: &Embedding,
    existing_points: &[DiskPoint],
    existing_embeddings: &[Embedding],
) -> anyhow::Result<f64> {
    let point = out_of_sample_project(embedding, existing_points, existing_embeddings)?;
    Ok(point.y.atan2(point.x))
}

/// Align `source` points to `target` via Procrustes analysis.
///
/// Finds the optimal rotation+translation that minimizes ||source*R + t - target||^2.
/// This ensures coordinate system continuity across incremental refinements.
fn procrustes_align(source: &[DiskPoint], target: &[DiskPoint]) -> anyhow::Result<Vec<DiskPoint>> {
    let n = source.len();
    if n == 0 || n != target.len() {
        anyhow::bail!("Procrustes requires equal-length non-empty arrays");
    }

    // Step 1: Center both sets
    let (src_centered, _src_mean) = center_points(source);
    let (tgt_centered, tgt_mean) = center_points(target);

    if n == 1 {
        return Ok(vec![tgt_mean]);
    }

    // Step 2: Compute cross-covariance H = source^T * target
    let h11: f64 = src_centered.iter().zip(&tgt_centered).map(|(s, t)| s.x * t.x).sum();
    let h12: f64 = src_centered.iter().zip(&tgt_centered).map(|(s, t)| s.x * t.y).sum();
    let h21: f64 = src_centered.iter().zip(&tgt_centered).map(|(s, t)| s.y * t.x).sum();
    let h22: f64 = src_centered.iter().zip(&tgt_centered).map(|(s, t)| s.y * t.y).sum();

    // Step 3: SVD of 2x2 H -- for 2D, use analytic formula
    // H = U Sigma V^T, optimal R = V U^T
    // For 2x2: compute via atan2 of the rotation angle
    let trace = h11 + h22;
    let anti_trace = h12 - h21; // off-diagonal difference
    let theta = anti_trace.atan2(trace);
    let cos_t = theta.cos();
    let sin_t = theta.sin();

    // Step 4: Apply rotation + translation
    let aligned: Vec<DiskPoint> = src_centered
        .iter()
        .map(|s| DiskPoint {
            x: cos_t * s.x - sin_t * s.y + tgt_mean.x,
            y: sin_t * s.x + cos_t * s.y + tgt_mean.y,
        })
        .collect();

    Ok(aligned)
}

/// Center a set of points, returning (centered_points, centroid).
fn center_points(points: &[DiskPoint]) -> (Vec<DiskPoint>, DiskPoint) {
    let n = points.len() as f64;
    let cx: f64 = points.iter().map(|p| p.x).sum::<f64>() / n;
    let cy: f64 = points.iter().map(|p| p.y).sum::<f64>() / n;
    let centered: Vec<DiskPoint> = points
        .iter()
        .map(|p| DiskPoint { x: p.x - cx, y: p.y - cy })
        .collect();
    (centered, DiskPoint { x: cx, y: cy })
}

/// Angle bisection: given a current angle theta_c and a new restoration angle
/// theta_new, compute the next search angle.
///
/// Standard bisection: theta_next = (theta_c + theta_new) / 2, then feed back to R(theta_next).
/// This is the core active search loop -- confidence feedback guides the
/// bisection toward density peaks.
pub fn angle_bisect(current: Angle, new: Angle) -> Angle {
    let bisect_deg = (current.degrees + new.degrees) / 2.0;
    Angle::from_degrees(bisect_deg)
}

/// Advanced bisection: choose the next angle based on confidence gradient.
///
/// If the confidence at theta_new is higher than at theta_c, bias the search
/// toward theta_new's quadrant. Confidence-weighted bisection converges
/// faster than uniform bisection.
pub fn confidence_weighted_bisect(
    current: Angle,
    new: Angle,
    current_conf: f64,
    new_conf: f64,
) -> Angle {
    let total = current_conf + new_conf;
    if total < f64::EPSILON {
        return angle_bisect(current, new);
    }
    let weight_current = current_conf / total;
    let weight_new = new_conf / total;
    let weighted_deg = current.degrees * weight_current + new.degrees * weight_new;
    Angle::from_degrees(weighted_deg)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bisect_is_symmetric() {
        let a = Angle::from_degrees(30.0);
        let b = Angle::from_degrees(150.0);
        let bisect = angle_bisect(a, b);
        assert!((bisect.degrees - 90.0).abs() < 1e-6);
    }

    #[test]
    fn confidence_weighted_leads_to_higher_confidence_side() {
        let a = Angle::from_degrees(30.0);
        let b = Angle::from_degrees(150.0);
        let result = confidence_weighted_bisect(a, b, 0.8, 0.2);
        // 30 * 0.8 + 150 * 0.2 = 24 + 30 = 54
        assert!((result.degrees - 54.0).abs() < 1e-6);
    }

    #[test]
    fn cosine_distance_matrix_orthogonal() {
        // Orthogonal vectors: cos_sim = 0, distance = 1
        let embs = vec![
            vec![1.0, 0.0, 0.0],
            vec![0.0, 1.0, 0.0],
            vec![0.0, 0.0, 1.0],
        ];
        let dist = cosine_distance_matrix(&embs).unwrap();
        assert!((dist[0][1] - 1.0).abs() < 1e-6);
        assert!((dist[0][2] - 1.0).abs() < 1e-6);
        assert!((dist[1][2] - 1.0).abs() < 1e-6);
    }

    #[test]
    fn cosine_distance_matrix_identical() {
        let embs = vec![
            vec![1.0, 2.0, 3.0],
            vec![2.0, 4.0, 6.0], // parallel -> cos_sim = 1, distance = 0
        ];
        let dist = cosine_distance_matrix(&embs).unwrap();
        assert!(dist[0][1].abs() < 1e-6);
    }

    #[test]
    fn cold_start_mds_three_points() {
        // Three maximally dissimilar embeddings (orthogonal)
        let embs = [
            vec![1.0, 0.0, 0.0],
            vec![0.0, 1.0, 0.0],
            vec![0.0, 0.0, 1.0],
        ];
        let points = cold_start_mds(&embs).unwrap();
        // All pairwise distances should be ~1.0 (cosine distance of orthogonal vectors)
        let d01 = points[0].cos_distance(&points[1]);
        let d02 = points[0].cos_distance(&points[2]);
        let d12 = points[1].cos_distance(&points[2]);
        // MDS should approximately preserve distances
        assert!(d01 > 0.5, "d01 = {}, expected > 0.5", d01);
        assert!(d02 > 0.5, "d02 = {}, expected > 0.5", d02);
        assert!(d12 > 0.5, "d12 = {}, expected > 0.5", d12);
    }

    #[test]
    fn cold_start_mds_non_degenerate() {
        // Ensure the 3 points are not all at the origin
        let embs = [
            vec![1.0, 0.0],
            vec![0.0, 1.0],
            vec![1.0, 1.0],
        ];
        let points = cold_start_mds(&embs).unwrap();
        let has_nonzero = points.iter().any(|p| p.x.abs() > 1e-6 || p.y.abs() > 1e-6);
        assert!(has_nonzero, "MDS produced all-zero coordinates");
    }

    #[test]
    fn out_of_sample_single_reference() {
        let existing_pts = vec![DiskPoint { x: 1.0, y: 2.0 }];
        let existing_embs = vec![vec![1.0, 0.0]];
        let new_emb = vec![0.0, 1.0]; // orthogonal -> distance = 1.0

        let projected = out_of_sample_project(&new_emb, &existing_pts, &existing_embs).unwrap();
        // Should be at distance ~1.0 from the existing point
        let dx = projected.x - existing_pts[0].x;
        let dy = projected.y - existing_pts[0].y;
        let dist = (dx * dx + dy * dy).sqrt();
        assert!((dist - 1.0).abs() < 0.1, "projected distance = {}, expected ~1.0", dist);
    }

    #[test]
    fn out_of_sample_two_references() {
        let existing_pts = vec![
            DiskPoint { x: 0.0, y: 0.0 },
            DiskPoint { x: 2.0, y: 0.0 },
        ];
        let existing_embs = vec![
            vec![1.0, 0.0, 0.0],
            vec![0.0, 1.0, 0.0],
        ];
        let new_emb = vec![0.0, 0.0, 1.0]; // equidistant from both -> distance = 1.0

        let projected = out_of_sample_project(&new_emb, &existing_pts, &existing_embs).unwrap();
        // Should be approximately at (1.0, +/-something) -- equidistant from (0,0) and (2,0)
        assert!((projected.x - 1.0).abs() < 0.2, "x = {}, expected ~1.0", projected.x);
    }

    #[test]
    fn incremental_refinement_preserves_structure() {
        let embs = vec![
            vec![1.0, 0.0, 0.0],
            vec![0.0, 1.0, 0.0],
            vec![0.0, 0.0, 1.0],
            vec![1.0, 1.0, 0.0],
            vec![1.0, 0.0, 1.0],
        ];
        // Initial points (arbitrary)
        let initial_points = vec![
            DiskPoint { x: 0.0, y: 0.0 },
            DiskPoint { x: 1.0, y: 0.0 },
            DiskPoint { x: 0.5, y: 0.866 },
            DiskPoint { x: 1.5, y: 0.866 },
            DiskPoint { x: 0.25, y: 0.433 },
        ];

        let refined = incremental_mds_refinement(&initial_points, &embs).unwrap();
        assert_eq!(refined.len(), 5);
        // Refined points should be non-degenerate
        let has_spread = refined.iter().any(|p| p.x.abs() > 0.1 || p.y.abs() > 0.1);
        assert!(has_spread, "refined coordinates lack spread");
    }

    #[test]
    fn jacobi_identity_matrix() {
        let identity = vec![
            vec![2.0, 0.0, 0.0],
            vec![0.0, 3.0, 0.0],
            vec![0.0, 0.0, 1.0],
        ];
        let (eigenvalues, _) = jacobi_eigen(&identity).unwrap();
        // Eigenvalues of diagonal matrix are the diagonal entries
        let mut sorted_eigs = eigenvalues.clone();
        sorted_eigs.sort_by(|a, b| a.partial_cmp(b).unwrap());
        assert!((sorted_eigs[0] - 1.0).abs() < 1e-6);
        assert!((sorted_eigs[1] - 2.0).abs() < 1e-6);
        assert!((sorted_eigs[2] - 3.0).abs() < 1e-6);
    }
}
