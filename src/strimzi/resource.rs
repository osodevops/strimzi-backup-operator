use kube::{
    api::{Api, ApiResource, DynamicObject, GroupVersionKind},
    Client,
};
use tracing::debug;

const STRIMZI_GROUP: &str = "kafka.strimzi.io";
const PREFERRED_API_VERSION: &str = "v1";
const LEGACY_API_VERSION: &str = "v1beta2";

/// Get a namespaced Strimzi resource using the stable API, falling back to the
/// legacy API served by Strimzi releases older than 0.49.
///
/// Strimzi 0.49 through 0.51 serve both versions, while Strimzi 1.0 serves only
/// `v1`. A 404 can mean either that the API version is not served or that the
/// named resource is absent, so retrying the legacy endpoint is safe. Other
/// errors (such as authorization or transport failures) are returned directly.
pub(crate) async fn get_namespaced_resource(
    client: &Client,
    namespace: &str,
    kind: &str,
    name: &str,
) -> Result<DynamicObject, kube::Error> {
    match get_at_version(client, namespace, kind, name, PREFERRED_API_VERSION).await {
        Ok(resource) => Ok(resource),
        Err(error) if is_not_found(&error) => {
            debug!(
                %kind,
                %name,
                %namespace,
                version = PREFERRED_API_VERSION,
                fallback_version = LEGACY_API_VERSION,
                "Strimzi resource not found at preferred API version; trying legacy API"
            );
            get_at_version(client, namespace, kind, name, LEGACY_API_VERSION).await
        }
        Err(error) => Err(error),
    }
}

async fn get_at_version(
    client: &Client,
    namespace: &str,
    kind: &str,
    name: &str,
    version: &str,
) -> Result<DynamicObject, kube::Error> {
    let resource = ApiResource::from_gvk(&GroupVersionKind::gvk(STRIMZI_GROUP, version, kind));
    let api: Api<DynamicObject> = Api::namespaced_with(client.clone(), namespace, &resource);
    api.get(name).await
}

fn is_not_found(error: &kube::Error) -> bool {
    matches!(error, kube::Error::Api(response) if response.code == 404)
}
