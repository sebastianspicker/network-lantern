use crate::Result;

/// Enforces the provider-inventory barrier before a mutation continuation is
/// constructed. Production uses this for QoS so an inventory failure cannot
/// reach registry, CIM, power, or publication mutations.
pub(crate) fn after_inventory<I, T>(
    inventory: impl FnOnce() -> Result<I>,
    mutate: impl FnOnce(I) -> Result<T>,
) -> Result<T> {
    mutate(inventory()?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::TuningError;
    use std::cell::Cell;

    #[test]
    fn qos_inventory_failure_performs_zero_mutations() {
        let mutated = Cell::new(false);
        let result: Result<()> = after_inventory(
            || -> Result<Vec<()>> {
                Err(TuningError::BackupInvalid(
                    "injected inventory failure".into(),
                ))
            },
            |_| {
                mutated.set(true);
                Ok(())
            },
        );
        assert!(result.is_err());
        assert!(!mutated.get());
    }
}
