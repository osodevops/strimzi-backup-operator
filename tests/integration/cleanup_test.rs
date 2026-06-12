use kube::api::PropagationPolicy;

use kafka_backup_operator::reconcilers::cleanup_delete_params;

/// Issue #30: deleting a KafkaRestore left its Job's pods behind. The batch/v1
/// Job API's legacy server-side default deletion propagation is `Orphan`, so
/// deleting a Job without an explicit propagation policy strips the Job
/// ownerReference from its pods instead of deleting them (kubectl masks this
/// by always sending Background). Cleanup deletes must request Background
/// propagation so dependents are garbage collected.
#[test]
fn test_cleanup_deletes_propagate_to_dependents() {
    let params = cleanup_delete_params();
    assert!(
        matches!(
            params.propagation_policy,
            Some(PropagationPolicy::Background)
        ),
        "cleanup must delete with Background propagation, got {:?}",
        params.propagation_policy
    );
}
