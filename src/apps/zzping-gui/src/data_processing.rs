//! Handles the conversion of network data into plottable data points.

use crate::data::DataPoint;
use chrono::{DateTime, Duration};
use zzping_lib::protocol::RawDataRecord;

/// Converts a vector of `RawDataRecord`s from the network into a vector of `DataPoint`s for plotting.
///
/// This function is responsible for:
/// 1. Sorting the records by their send timestamp, as the database does not guarantee order.
/// 2. Converting the `u64` nanosecond timestamps into strongly-typed `chrono::DateTime<Utc>` objects.
/// 3. Converting the `u64` RTT values into `chrono::Duration`, handling the `u64::MAX` sentinel for lost packets.
///
/// The plot widget expects absolute timestamps and calculates the relative view itself, so this
/// function does not perform any time normalization.
pub fn records_to_points(records: Vec<RawDataRecord>) -> Vec<DataPoint> {
    if records.is_empty() {
        return Vec::new();
    }

    let mut sorted_records = records;
    sorted_records.sort_by_key(|r| r.sent_nanos);

    let points: Vec<DataPoint> = sorted_records
        .into_iter()
        .map(|rec| DataPoint {
            time: DateTime::from_timestamp_nanos(rec.sent_nanos as i64),
            rtt: if rec.rtt_nanos == u64::MAX {
                None
            } else {
                Some(Duration::nanoseconds(rec.rtt_nanos as i64))
            },
        })
        .collect();

    points
}

#[cfg(test)]
mod tests {
    use super::*;
    // Removed use ntest::timeout;

    #[test]
    // Removed #[timeout(100)]
    fn test_records_to_points_conversion() {
        let records = vec![
            RawDataRecord {
                sent_nanos: 2_000_000_000,
                rtt_nanos: 20_000_000,
            },
            RawDataRecord {
                sent_nanos: 1_000_000_000, // Unsorted
                rtt_nanos: 10_000_000,
            },
            RawDataRecord {
                sent_nanos: 3_000_000_000,
                rtt_nanos: u64::MAX, // Lost
            },
        ];

        let points = records_to_points(records);

        assert_eq!(points.len(), 3);

        // Check that they are sorted and time is absolute
        assert_eq!(
            points[0].time,
            DateTime::from_timestamp_nanos(1_000_000_000)
        );
        assert_eq!(
            points[1].time,
            DateTime::from_timestamp_nanos(2_000_000_000)
        );
        assert_eq!(
            points[2].time,
            DateTime::from_timestamp_nanos(3_000_000_000)
        );

        // Check RTT conversion
        assert_eq!(points[0].rtt, Some(Duration::nanoseconds(10_000_000)));
        assert_eq!(points[1].rtt, Some(Duration::nanoseconds(20_000_000)));
        assert_eq!(points[2].rtt, None);
    }

    #[test]
    // Removed #[timeout(100)]
    fn test_records_to_points_empty() {
        let records = Vec::new();
        let points = records_to_points(records);
        assert!(points.is_empty());
    }
}
