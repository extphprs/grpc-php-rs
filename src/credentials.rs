use ext_php_rs::prelude::*;
use ext_php_rs::types::Zval;
use parking_lot::Mutex;
use std::sync::Arc;
use tonic::transport::ClientTlsConfig;

static DEFAULT_ROOTS_PEM: Mutex<Option<String>> = Mutex::new(None);

/// Internal representation of TLS configuration.
#[derive(Debug, Clone)]
pub enum CredentialsInner {
    Ssl {
        tls_config: ClientTlsConfig,
    },
    Composite {
        tls_config: ClientTlsConfig,
        call_plugin: Arc<Mutex<Option<Zval>>>,
    },
}

#[php_class]
#[php(name = "Grpc\\ChannelCredentials")]
pub struct GrpcChannelCredentials {
    pub(crate) inner: CredentialsInner,
    /// Distinguishes credential objects in the persistent-channel key —
    /// channels built from different credentials must never share a
    /// transport (C-core keys on a credentials hash).
    pub(crate) id: u64,
}

/// Monotonic id source for [`GrpcChannelCredentials::id`].
pub(crate) fn next_credentials_id() -> u64 {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
    NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
}

#[php_impl]
impl GrpcChannelCredentials {
    #[php(name = "createSsl")]
    pub fn create_ssl(
        pem_root_certs: Option<&Zval>,
        pem_private_key: Option<&Zval>,
        pem_cert_chain: Option<&Zval>,
    ) -> PhpResult<Self> {
        let msg = "createSsl expects 3 optional strings";
        let parse = |arg: Option<&Zval>| match arg {
            None => Ok(None),
            Some(zv) => crate::args::nullable_string(zv)
                .ok_or_else(|| crate::error::invalid_argument(msg)),
        };
        let pem_root_certs = parse(pem_root_certs)?;
        let pem_private_key = parse(pem_private_key)?;
        let pem_cert_chain = parse(pem_cert_chain)?;

        // Always enable webpki-roots (Mozilla CA bundle) as fallback.
        // tonic does NOT auto-enable it even with the `tls-webpki-roots` feature.
        let mut tls = ClientTlsConfig::new().with_webpki_roots();

        // Apply root certs: parameter takes priority, then default, then system roots
        let roots = pem_root_certs.or_else(|| {
            let guard = DEFAULT_ROOTS_PEM.lock();
            guard.clone()
        });
        if let Some(pem) = roots {
            let cert = tonic::transport::Certificate::from_pem(pem);
            tls = tls.ca_certificate(cert);
        }

        // Apply client identity (mutual TLS)
        if let (Some(cert_chain), Some(private_key)) = (pem_cert_chain, pem_private_key) {
            let identity = tonic::transport::Identity::from_pem(cert_chain, private_key);
            tls = tls.identity(identity);
        }

        Ok(Self {
            inner: CredentialsInner::Ssl { tls_config: tls },
            id: next_credentials_id(),
        })
    }

    /// Creates insecure credentials.
    ///
    /// MUST return PHP null for drop-in compatibility.
    /// The Channel constructor checks `credentials === null` to mean insecure.
    #[php(name = "createInsecure")]
    pub fn create_insecure() -> Option<Self> {
        None
    }

    #[php(name = "createComposite")]
    pub fn create_composite(channel_creds: &Zval, call_creds: &Zval) -> PhpResult<Self> {
        let channel_creds = channel_creds
            .extract::<&GrpcChannelCredentials>()
            .ok_or_else(|| crate::error::invalid_argument("createComposite expects 2 Credentials"))?;
        let call_creds = call_creds
            .extract::<&GrpcCallCredentials>()
            .ok_or_else(|| crate::error::invalid_argument("createComposite expects 2 Credentials"))?;

        let tls_config = match &channel_creds.inner {
            CredentialsInner::Ssl { tls_config } => tls_config.clone(),
            CredentialsInner::Composite { tls_config, .. } => tls_config.clone(),
        };

        Ok(Self {
            id: next_credentials_id(),
            inner: CredentialsInner::Composite {
                tls_config,
                call_plugin: Arc::clone(&call_creds.plugin),
            },
        })
    }

    /// Creates default SSL credentials (system root CAs or custom default PEM).
    #[php(name = "createDefault")]
    pub fn create_default() -> Self {
        let mut tls = ClientTlsConfig::new().with_webpki_roots();

        let guard = DEFAULT_ROOTS_PEM.lock();
        if let Some(ref pem) = *guard {
            let cert = tonic::transport::Certificate::from_pem(pem.clone());
            tls = tls.ca_certificate(cert);
        }
        drop(guard);

        Self {
            inner: CredentialsInner::Ssl { tls_config: tls },
            id: next_credentials_id(),
        }
    }

    #[php(name = "setDefaultRootsPem")]
    pub fn set_default_roots_pem(pem: &Zval) -> PhpResult<()> {
        let pem = crate::args::string(pem).ok_or_else(|| {
            crate::error::invalid_argument("setDefaultRootsPem expects 1 string")
        })?;
        let mut guard = DEFAULT_ROOTS_PEM.lock();
        *guard = Some(pem);
        Ok(())
    }

    /// Checks if default root PEM has been set.
    #[php(name = "isDefaultRootsPemSet")]
    pub fn is_default_roots_pem_set() -> bool {
        let guard = DEFAULT_ROOTS_PEM.lock();
        guard.is_some()
    }

    /// Invalidates the default root PEM.
    #[php(name = "invalidateDefaultRootsPem")]
    pub fn invalidate_default_roots_pem() {
        let mut guard = DEFAULT_ROOTS_PEM.lock();
        *guard = None;
    }
}

// ---------------------------------------------------------------------------
// CallCredentials
// ---------------------------------------------------------------------------

#[php_class]
#[php(name = "Grpc\\CallCredentials")]
pub struct GrpcCallCredentials {
    pub(crate) plugin: Arc<Mutex<Option<Zval>>>,
}

#[php_impl]
impl GrpcCallCredentials {
    #[php(name = "createFromPlugin")]
    pub fn create_from_plugin(callback: &Zval) -> PhpResult<Self> {
        if !callback.is_callable() {
            return Err(crate::error::invalid_argument(
                "createFromPlugin expects 1 callback",
            ));
        }

        // Zval is !Send, but Arc is needed for shared ownership across PHP objects.
        // The callable is ONLY invoked on the PHP thread (before block_on).
        #[allow(clippy::arc_with_non_send_sync)]
        Ok(Self {
            plugin: Arc::new(Mutex::new(Some(callback.shallow_clone()))),
        })
    }

    #[php(name = "createComposite")]
    pub fn create_composite(creds1: &Zval, creds2: &Zval) -> PhpResult<Self> {
        let creds1 = creds1.extract::<&GrpcCallCredentials>().ok_or_else(|| {
            crate::error::invalid_argument("createComposite expects 2 CallCredentials")
        })?;
        let creds2 = creds2.extract::<&GrpcCallCredentials>().ok_or_else(|| {
            crate::error::invalid_argument("createComposite expects 2 CallCredentials")
        })?;

        let plugin = {
            let guard = creds1.plugin.lock();
            guard.as_ref().map(|z| z.shallow_clone())
        };

        let plugin = plugin.or_else(|| {
            let guard = creds2.plugin.lock();
            guard.as_ref().map(|z| z.shallow_clone())
        });

        #[allow(clippy::arc_with_non_send_sync)]
        Ok(Self {
            plugin: Arc::new(Mutex::new(plugin)),
        })
    }
}
