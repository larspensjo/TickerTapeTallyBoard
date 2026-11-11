use super::*;
use rust_decimal_macros::dec;

fn calibrated() -> Calibration {
    Calibration {
        outcome: Sufficiency::Calibrated,
        real: true,
    }
}
fn sample() -> Comparison {
    Comparison {
        events: 30,
        instruments: 8,
        improvements: [Some(dec!(1)), Some(dec!(0.1)), Some(dec!(0.1))],
    }
}
fn candidate(value: Decimal, gain: Decimal) -> GridValue {
    let mut s = sample();
    s.improvements[0] = Some(gain);
    GridValue {
        value,
        windows: [s; 3],
    }
}

#[test]
fn change_has_exact_gain_size_and_positive_half_boundaries() {
    let c = calibrated();
    assert_eq!(change(c, sample()), Criterion::Met);
    for (gain, events, instruments, first, second) in [
        (dec!(0.999999), 30, 8, dec!(1), dec!(1)),
        (dec!(1), 29, 8, dec!(1), dec!(1)),
        (dec!(1), 30, 7, dec!(1), dec!(1)),
        (dec!(1), 30, 8, dec!(0), dec!(1)),
        (dec!(1), 30, 8, dec!(1), dec!(0)),
        (dec!(1), 30, 8, dec!(-0.01), dec!(1)),
        (dec!(1), 30, 8, dec!(1), dec!(-0.01)),
    ] {
        assert_eq!(
            change(
                c,
                Comparison {
                    events,
                    instruments,
                    improvements: [Some(gain), Some(first), Some(second)]
                }
            ),
            Criterion::NotMet
        );
    }
    for index in 0..3 {
        let mut s = sample();
        s.improvements[index] = None;
        assert_eq!(change(c, s), Criterion::Undetermined);
    }
}

#[test]
fn same_value_must_qualify_in_every_window() {
    let mut v = candidate(dec!(2), dec!(1));
    v.windows[2].improvements[0] = Some(dec!(0.99));
    assert_eq!(
        choose_value(calibrated(), dec!(1.5), &[v]).criterion,
        Criterion::NotMet
    );
    let mut v = candidate(dec!(2), dec!(1));
    v.windows[1].improvements[1] = None;
    assert_eq!(
        choose_value(calibrated(), dec!(1.5), &[v]).criterion,
        Criterion::Undetermined
    );
    // Different values winning different windows cannot combine into a winner.
    let mut a = candidate(dec!(1), dec!(2));
    let mut b = candidate(dec!(2), dec!(2));
    a.windows[0].improvements[0] = Some(dec!(0));
    b.windows[1].improvements[0] = Some(dec!(0));
    assert!(choose_value(calibrated(), dec!(1.5), &[a, b])
        .chosen
        .is_none());
}

#[test]
fn value_selection_uses_30d_whole_gain_then_distance_then_smaller_value() {
    let mut a = candidate(dec!(1), dec!(2));
    let mut b = candidate(dec!(2), dec!(1));
    b.windows[0].improvements[0] = Some(dec!(99));
    assert_eq!(
        choose_value(calibrated(), dec!(1.5), &[a, b]).chosen,
        Some((dec!(1), dec!(2)))
    );
    a = candidate(dec!(1), dec!(2));
    b = candidate(dec!(2), dec!(2));
    assert_eq!(
        choose_value(calibrated(), dec!(1.8), &[a, b]).chosen,
        Some((dec!(2), dec!(2)))
    );
    // Reverse iteration to prove the numeric tie-break, independent of grid order.
    assert_eq!(
        choose_value(
            calibrated(),
            dec!(1.5),
            &[candidate(dec!(2), dec!(2)), candidate(dec!(1), dec!(2))]
        )
        .chosen,
        Some((dec!(1), dec!(2)))
    );
}

#[test]
fn one_change_per_replay_and_equal_cross_grid_gain_uses_registered_order() {
    let grids = [
        choose_value(calibrated(), dec!(1.5), &[candidate(dec!(2), dec!(1))]),
        choose_value(calibrated(), dec!(20), &[candidate(dec!(25), dec!(2))]),
        choose_value(calibrated(), dec!(2), &[candidate(dec!(3), dec!(2))]),
    ];
    assert_eq!(choose_one(calibrated(), &grids), Some(1));
    assert!(grids[0].chosen.is_some() && grids[2].chosen.is_some());
}

#[test]
fn drop_needs_every_value_both_halves_and_every_window() {
    let c = calibrated();
    let clean = [[Some(dec!(0)), Some(dec!(-1))]; 4];
    assert_eq!(drop_window(c, &clean), Criterion::Met);
    assert_eq!(all_windows([drop_window(c, &clean); 3]), Criterion::Met);
    for value in 0..4 {
        for half in 0..2 {
            let mut rows = clean;
            rows[value][half] = Some(dec!(0.000001));
            assert_eq!(drop_window(c, &rows), Criterion::NotMet);
            assert_eq!(
                all_windows([Criterion::Met, Criterion::Met, drop_window(c, &rows)]),
                Criterion::NotMet
            );
            rows[value][half] = None;
            assert_eq!(drop_window(c, &rows), Criterion::Undetermined);
            assert_eq!(
                all_windows([Criterion::Met, Criterion::Met, drop_window(c, &rows)]),
                Criterion::Undetermined
            );
        }
    }
    assert_eq!(drop_window(c, &[]), Criterion::Undetermined);
}

#[test]
fn insufficient_history_and_demo_never_authorize_changes_or_drop_proposals() {
    let qualified = [GridDecision {
        criterion: Criterion::Met,
        chosen: Some((dec!(2), dec!(2))),
    }];
    for c in [
        Calibration {
            outcome: Sufficiency::InsufficientHistory,
            real: true,
        },
        Calibration {
            outcome: Sufficiency::Calibrated,
            real: false,
        },
    ] {
        assert_eq!(change(c, sample()), Criterion::Undetermined);
        assert_eq!(
            choose_value(c, dec!(1.5), &[candidate(dec!(2), dec!(2))]).criterion,
            Criterion::Undetermined
        );
        assert_eq!(choose_one(c, &qualified), None);
        assert_eq!(
            drop_window(c, &[[Some(dec!(-1)); 2]; 4]),
            Criterion::Undetermined
        );
    }
}

#[test]
fn default_lookup_is_numeric_and_outside_grid_is_fixed_text_error() {
    assert_eq!(
        default_index(&[dec!(1.0), dec!(1.50), dec!(2)], dec!(1.500)),
        Ok(1)
    );
    assert_eq!(
        default_index(&[dec!(1), dec!(2)], dec!(1.5)),
        Err("Replay default threshold is outside its registered grid.")
    );
}
