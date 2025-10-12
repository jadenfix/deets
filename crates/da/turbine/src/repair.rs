pub fn missing_indices(total: usize, present: &[u32]) -> Vec<u32> {
    let mut present_set = present.to_vec();
    present_set.sort_unstable();
    present_set.dedup();

    (0..total as u32)
        .filter(|idx| !present_set.contains(idx))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn computes_missing_indices() {
        let missing = missing_indices(5, &[0, 2]);
        assert_eq!(missing, vec![1, 3, 4]);
    }
}
