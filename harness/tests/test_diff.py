from harness.diff import compare_dlq, compare_records


def test_record_diff_reports_missing_extra_duplicates_and_changed_fields():
    result = compare_records(
        [
            {"session_id": "same", "count": 1},
            {"session_id": "missing", "count": 2},
        ],
        [
            {"session_id": "same", "count": 3},
            {"session_id": "same", "count": 4},
            {"session_id": "extra", "count": 5},
        ],
        "session_id",
    )
    assert result["missing"] == [
        {"key": "missing", "golden": {"session_id": "missing", "count": 2}}
    ]
    assert result["extra"] == [{"key": "extra", "processor": {"session_id": "extra", "count": 5}}]
    assert result["duplicate"] == ["same"]
    assert result["changed"] == [
        {
            "key": "same",
            "fields": [{"field": "count", "golden": 1, "processor": 3}],
        }
    ]


def test_record_diff_marks_fields_absent_on_either_side():
    result = compare_records(
        [{"event_id": "event", "stable": True}],
        [{"event_id": "event", "end_location": {"lat": 1}}],
        "event_id",
    )
    fields = {item["field"]: item for item in result["changed"][0]["fields"]}
    assert fields["stable"] == {
        "field": "stable",
        "golden": True,
        "processor": "<absent>",
    }
    assert fields["end_location"] == {
        "field": "end_location",
        "golden": "<absent>",
        "processor": {"lat": 1},
    }


def test_record_diff_handles_non_hashable_candidate_keys():
    result = compare_records(
        [{"event_id": ["invalid"], "value": 1}],
        [{"event_id": ["invalid"], "value": 1}],
        "event_id",
    )
    assert result["identical"] == 1
    assert result["duplicate"] == []


def test_float_tolerance_and_boolean_values_are_not_numeric():
    within = compare_records(
        [{"event_id": "a", "value": 4.0}],
        [{"event_id": "a", "value": 4.0000009}],
        "event_id",
    )
    outside = compare_records(
        [{"event_id": "a", "value": 4.0}],
        [{"event_id": "a", "value": 4.000002}],
        "event_id",
    )
    boolean = compare_records(
        [{"event_id": "a", "value": True}],
        [{"event_id": "a", "value": 1}],
        "event_id",
    )
    assert within["identical"] == 1
    assert outside["changed"]
    assert boolean["changed"]


def test_dlq_multiset_counts_duplicate_pairs_and_distinct_reasons():
    result = compare_dlq(
        [
            {"event_id": "same", "reason": "BAD"},
            {"event_id": "same", "reason": "BAD"},
            {"event_id": "same", "reason": "OTHER"},
            {"event_id": None, "reason": "NULL_ID"},
        ],
        [
            {"event_id": "same", "reason": "BAD"},
            {"event_id": "same", "reason": "OTHER"},
            {"event_id": None, "reason": "NULL_ID"},
            {"event_id": None, "reason": "NEW"},
        ],
    )
    assert result["missing"] == [{"event_id": "same", "reason": "BAD", "count": 1}]
    assert result["extra"] == [{"event_id": None, "reason": "NEW", "count": 1}]


def test_dlq_missing_pair_fields_are_reported_as_null():
    result = compare_dlq(
        [],
        [{"event_id": "missing-reason"}, {"reason": "missing-event-id"}],
    )

    assert result["extra"] == [
        {"event_id": "missing-reason", "reason": None, "count": 1},
        {"event_id": None, "reason": "missing-event-id", "count": 1},
    ]
