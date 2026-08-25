use recursa_codegen::{KindId, KindSet, KindSetError};

#[test]
fn dynamic_kind_sets_preserve_exact_width_and_ascending_iteration() {
    let set = KindSet::from_kinds(65, [KindId::new(64), KindId::new(1), KindId::new(63)]).unwrap();

    assert_eq!(set.kind_count(), 65);
    assert_eq!(set.words(), &[0x8000_0000_0000_0002, 1]);
    assert_eq!(
        set.iter().collect::<Vec<_>>(),
        [KindId::new(1), KindId::new(63), KindId::new(64)],
    );
}

#[test]
fn kind_set_algebra_rejects_width_mismatches_and_clears_tails() {
    let left = KindSet::from_kinds(65, [KindId::new(0), KindId::new(64)]).unwrap();
    let right = KindSet::from_kinds(65, [KindId::new(1), KindId::new(64)]).unwrap();

    assert_eq!(
        left.union(&right).unwrap().iter().collect::<Vec<_>>(),
        [KindId::new(0), KindId::new(1), KindId::new(64)],
    );
    assert_eq!(
        left.intersection(&right)
            .unwrap()
            .iter()
            .collect::<Vec<_>>(),
        [KindId::new(64)],
    );
    assert_eq!(
        left.difference(&right).unwrap().iter().collect::<Vec<_>>(),
        [KindId::new(0)],
    );
    assert!(KindSet::empty(65).is_empty());
    assert_eq!(
        left.union(&KindSet::empty(64)),
        Err(KindSetError::WidthMismatch {
            left: 65,
            right: 64
        }),
    );
    assert_eq!(
        KindSet::singleton(1, KindId::new(1)),
        Err(KindSetError::KindOutOfRange {
            kind: KindId::new(1),
            kind_count: 1,
        }),
    );
}
