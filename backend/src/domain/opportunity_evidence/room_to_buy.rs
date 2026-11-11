use crate::domain::conviction::TargetStatus;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RoomToBuy {
    RoomToBuy,
    AlreadyAtTarget,
    NoTarget,
    Unknown,
}

pub fn room_to_buy(status: TargetStatus) -> RoomToBuy {
    match status {
        TargetStatus::Below => RoomToBuy::RoomToBuy,
        TargetStatus::Above | TargetStatus::OnTarget => RoomToBuy::AlreadyAtTarget,
        TargetStatus::NoTarget => RoomToBuy::NoTarget,
        TargetStatus::ExcludedUnavailable | TargetStatus::Unavailable => RoomToBuy::Unknown,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn every_target_status_maps_without_using_the_gap() {
        for (input, expected) in [
            (TargetStatus::Below, RoomToBuy::RoomToBuy),
            (TargetStatus::Above, RoomToBuy::AlreadyAtTarget),
            (TargetStatus::OnTarget, RoomToBuy::AlreadyAtTarget),
            (TargetStatus::NoTarget, RoomToBuy::NoTarget),
            (TargetStatus::ExcludedUnavailable, RoomToBuy::Unknown),
            (TargetStatus::Unavailable, RoomToBuy::Unknown),
        ] {
            assert_eq!(room_to_buy(input), expected);
        }
    }
}
