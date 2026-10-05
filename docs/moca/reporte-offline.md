# REPORTE OFFLINE — Inventario de llamadas al servidor y superficies que exigen login

Cliente Warp (Rust), binario GUI `app/src/bin/oss.rs` (raíz: `app/src/lib.rs`).
Escenario asumido: `server_root_url = http://127.0.0.1:1` (conexión rechazada) y login no obligatorio.
Todas las referencias son `archivo:línea` sobre el árbol actual. Lo no verificado se marca explícitamente.

---

## 1. Llamadas al servidor AL ARRANCAR (sin acción del usuario)

Flujo de arranque: `app/src/lib.rs:1311` (`app_builder.run`) → `initialize_app` (`app/src/lib.rs:1464`) → `RootView::new` (`app/src/root_view.rs:1890`+).

### 1a. Autenticación de arranque (solo si hay credenciales persistidas o API key)
- `app/src/lib.rs:1516-1524`: `AuthState::initialize(ctx)` lee credenciales del secure storage (sin red). `is_logged_in()` es puramente local: `crates/warp_server_auth/src/auth_state.rs:347-349` (`credentials.read().is_some()`).
- `app/src/lib.rs:1377-1388`: `StartupUserAuthentication::start` → `AuthManager::refresh_user` (`app/src/auth/auth_manager.rs:302-317`) → `auth_client.fetch_user(token, for_refresh=true)` → HTTP/GraphQL al server (pide usuario, experimentos, LLMs, credenciales frescas).
  - Si hay refresh token persistido, esta llamada **sí ocurre al arrancar** y fallará al instante (conexión rechazada). La app NO se tumba: ver manejo en §4.1.
  - Alternativa API key: `auth_manager.authenticate_api_key` (`app/src/auth/auth_manager.rs:324-340`).
- Si NO hay credenciales: no hay `fetch_user` en arranque. En su lugar `app/src/lib.rs:1986-1994` manda `TelemetryEvent::LoggedOutStartup` (Rudderstack, no warp-server) y `download_method::determine_and_report`.
- `app/src/root_view.rs:1929-1962`: la pantalla inicial se decide SOLO con el estado local (`is_logged_in()`): credenciales persistidas → workspace directo; sin ellas → Auth/Onboarding, salvo `FeatureFlag::SkipFirebaseAnonymousUser` (→ workspace directo, `app/src/root_view.rs:1953-1956`) o `ForceLogin` (→ login, `app/src/root_view.rs:1937-1939`).
- Punto crítico del login no obligatorio: `requires_post_onboarding_login` (`app/src/root_view.rs:634-641`) — tras onboarding exige login si `AccountFirstOnboarding` está activo O si el usuario eligió AI o Warp Drive en onboarding (`app/src/root_view.rs:2675-2684`).

### 1b. Telemetría (Rudderstack, no warp-server, pero es red al arrancar)
- `app/src/lib.rs:2084-2091`: registro de `TelemetryCollector` → `initialize_telemetry_collection` (`app/src/server/telemetry/collector.rs:44-78`): flush de eventos persistidos en disco al arrancar (línea 49), señales "Active Usage" cada 60 s (`collector.rs:20`) y flush de cola cada 30 s (`collector.rs:23`). Solo en release bundle o con flags de telemetría (`collector.rs:46-64`). Fallos: ignorados con log (§4.7).

### 1c. Autoupdate
- `app/src/root_view.rs:2061` + `2069-2073`: `AutoupdateState::start_polling` (`app/src/autoupdate/mod.rs:137-157`) arranca al crear el RootView (si ya estás en estado Terminal). El primer check va al server **inmediatamente al arrancar** (`poll_for_update` → `check_for_update` → `fetch_version` → `/client_version`, `app/src/autoupdate/mod.rs:215-223` y `266-280`). Con el server caído falla y sigue el loop (§4.2).
- Solo con `FeatureFlag::Autoupdate.is_enabled()` (`app/src/autoupdate/mod.rs:141`).

### 1d. Polling de cloud objects / metadata de workspaces (NO ocurren logged-out)
- Solo se arrancan tras un login exitoso: `on_user_fetched` → `TeamTesterStatus::initiate_data_pollers` (`app/src/auth/auth_manager.rs:456-461`) → `UpdateManager` (`app/src/server/cloud_objects/update_manager.rs:307-319`) y `TeamUpdateManager` (`app/src/workspaces/update_manager.rs:99-111`).
- Guardas anti logged-out: `app/src/server/cloud_objects/update_manager.rs:686-691` (aborta el poll si `!is_logged_in()`), `app/src/workspaces/update_manager.rs:159-163` y `:215` (salta refresh). Con login no obligatorio y sin credenciales, **no hay llamadas de este tipo al arrancar**.
- Si hay objetos no-welcome cacheados localmente (sqlite) del arranque anterior, el **websocket** sí puede intentar conectar: `Listener::new` (`app/src/server/cloud_objects/listener.rs:119-121`) y `start_listener` (`listener.rs:235-240`). Conexión rechazada → reintentos acotados (§4.4).

### 1e. Modelos LLM (server) — reactivo en producción
- `app/src/ai/llms.rs:894-899`: en producción NO se pide la lista de modelos al arrancar (solo en builds `agent_mode_evals`); se pide en eventos (login completo, red online, onboarding). Logged-out puede igualmente pedirse el catálogo público para el picker de onboarding: `refresh_public_models` (`app/src/ai/llms.rs:2003-2021`, "No auth required") vía `refresh_available_models` (`app/src/ai/llms.rs:2023-2033`), disparado p. ej. en `refresh_onboarding_account_state` (`app/src/root_view.rs:177-185`) y `app/src/root_view.rs:2189`.
- Nota: la lista authed no se pide si no hay login (`app/src/ai/llms.rs:1974-1977`).

### 1f. Cuota de AI
- `app/src/ai_assistant/requests.rs:133-141`: al construir el modelo, si no hay cuota cacheada Y `is_logged_in()` → `get_request_limit_info()` al server (ocurre al arrancar si hay sesión persistida).
- `app/src/ai/request_usage_model.rs:283-291`: `refresh_request_usage` manda `None` inmediatamente si `!is_logged_in()` (sin red).

### 1g. Otras llamadas al arrancar (login exitoso)
- `on_user_fetched` exitoso además dispara: `PrivacySettings::fetch_or_update_settings` (`app/src/auth/auth_manager.rs:516-522`), `notify_login` → `POST {server}/client/login` (`app/src/auth/auth_manager.rs:565`; `app/src/server/server_api.rs:1048-1051`), flush de telemetría de login (`app/src/auth/auth_manager.rs:559-564`), `SharedSessionManager.rejoin_all_shared_sessions` (`auth_manager.rs:488-493`), y daily update check (`auth_manager.rs:524-529`). Todas fallarán con el server caído pero ninguna bloquea la UI (async, error → log/Sentry).
- `ChangelogModel::new(server_api)` se registra en `app/src/lib.rs:2144` — si hace fetch al abrirse: **no verificado** (no encontrado en el presupuesto de búsqueda).

---

## 2. Llamadas periódicas / polling (timers, intervalos, reintentos)

| Polling | Cada cuánto | Dónde | Gate |
|---|---|---|---|
| Autoupdate `/client_version` | 10 min (`AUTOUPDATE_POLL`) | `app/src/autoupdate/mod.rs:211` (const), `:215-223` (loop autorrecursivo) | `FeatureFlag::Autoupdate` + `can_autoupdate` (`autoupdate/mod.rs:141`); además daily check al enfocar la app (`autoupdate/mod.rs:147-155`) y al login (`app/src/auth/auth_manager.rs:524-529`) |
| Cloud objects `fetch_changed_objects` | 10 min (`PERIODIC_POLL`) | `app/src/server/retry_strategies.rs:20` (const); usado en `app/src/server/cloud_objects/update_manager.rs:725` | Solo logged-in (`update_manager.rs:686-691`); se rearranca al volver online (`update_manager.rs:626-633` con `NetworkStatus`) |
| Workspace metadata `workspaces_metadata` | 10 min (`PERIODIC_POLL`) | `app/src/workspaces/update_manager.rs:243` (programa el próximo con `PERIODIC_POLL`); arranque en `:139-145` | Solo logged-in (`:159-163`, `:215`); reacciona a online/offline (`:81-97`) |
| Websocket RTC de cloud objects | Reconexión diferida: espera ≥30 s entre conexiones exitosas (`WAIT_PERIOD_BETWEEN_SUCCESSFUL_RECONNECTS`) | `app/src/server/cloud_objects/listener.rs:24` (const), `:359-365` | Solo si hay team u objetos no-welcome (`listener.rs:102-121`, `:133-149`) |
| Flush de telemetría (Rudderstack) | 30 s | `app/src/server/telemetry/collector.rs:23` | Release bundle o flags (`collector.rs:60-65`) |
| "Active usage" (Rudderstack) | 60 s | `app/src/server/telemetry/collector.rs:20` | `collector.rs:53-57` |
| Tareas de agentes ambientales (AI) | 30 s con jitter 0.2 (`POLLING_INTERVAL`) | `app/src/ai/agent_conversations_model.rs:66` (const), `:1213-1227` (loop), `:1242-1267` | Requiere contexto de team/usuario (poll se aborta si no hay resolver, `:1234-1236`) |
| Reintentos de operaciones de objetos (sync queue) | Backoff exponencial 500 ms ×2, máx 3 intentos | `app/src/server/retry_strategies.rs:112-138` (`MAX_ATTEMPTS`, `backoff_after_attempts`); p. ej. `app/src/server/sync_queue.rs:1128`, `:1214`, `:1454` | — |

Estrategias de retry (todas acotadas, `app/src/server/retry_strategies.rs`):
- `PERIODIC_POLL_RETRY_STRATEGY`: exponencial 2 s ×2, máx 3, jitter 0.2 (`:24-29`).
- `OUT_OF_BAND_REQUEST_RETRY_STRATEGY`: exponencial 100 ms ×5, máx 3, jitter 0.5 (`:34-39`).
- `LISTENER_RETRY_STRATEGY`: lineal 25 s, máx 5, jitter 0.6 (`:42-46`).
- `with_bounded_retry` / `with_bounded_retry_using` / `with_retry` (`:151-246`): bucle con presupuesto fijo de intentos; clasificación transitorio vs permanente (`:53-93`).

**No se encontró ningún reintento infinito.** Todos los bucles de retry tienen `max retry count` explícito.

---

## 3. Pantallas/acciones de UI que exigen login

`is_logged_in` = credenciales presentes (`crates/warp_server_auth/src/auth_state.rs:347-349`); `is_anonymous_or_logged_out` = sin credenciales o usuario anónimo (`auth_state.rs:357-359`).

| Pantalla / acción | Archivo:línea | Qué pasa sin login |
|---|---|---|
| Login slide / Auth view (onboarding) | `app/src/root_view.rs:1929-1962` (selección de estado); `app/src/auth/login_slide.rs:1119` (botón "Sign in"); `app/src/auth/auth_view_body.rs:497` | Sin credenciales la app entra a Auth/Onboarding salvo `SkipFirebaseAnonymousUser` (`root_view.rs:1953-1956`). Si `ForceLogin` (Preview) siempre exige login (`root_view.rs:1937-1939`) — con server caído el navegador no cargará la URL de login, pero la app sigue viva |
| Login post-onboarding | `app/src/root_view.rs:634-641` (`requires_post_onboarding_login`), usado en `:2681-2684` | Si `AccountFirstOnboarding` activo, o el usuario eligió AI/Drive en onboarding, se muestra el login slide después de onboarding. Con "skip login" (`app/src/auth/login_slide.rs:497-498` emite `SkippedLogin`) se completa sin red |
| Warp Drive (panel/habilitación) | `app/src/drive/settings.rs:48-61` (`is_warp_drive_available` / `is_warp_drive_enabled`) | Drive se considera deshabilitado para anónimo/logged-out aunque el setting local esté en `true` |
| Warp Drive — acciones de equipo/objetos | `app/src/drive/index.rs:5338-5347` (`handle_action`: gate `is_anonymous_or_logged_out` + `blocked_for_anonymous_user`) | Abre modal de login (`AuthViewVariant::RequireLoginCloseable`) vía `attempt_login_gated_feature` (`app/src/auth/auth_manager.rs:730-743`); la acción no se ejecuta |
| Warp Drive — compartir objeto | `app/src/drive/index.rs:5000-5009` (`toggle_share_dialog`) | Modal `ShareRequirementCloseable`; no abre el diálogo de share |
| Panel izquierdo (tool panels que requieren cuenta) | `app/src/workspace/view/left_panel.rs:317-331` (`ToolPanelAvailability::RequiresAccount` → botón "Sign in") | El panel muestra botón "Sign in" en vez del contenido; clic → `LeftPanelAction::SignIn` |
| AI (todas las superficies: `#`, Agent Mode, panel AI) | `app/src/settings/ai.rs:2266-2275` (`is_any_ai_enabled`: `false` si `is_anonymous_or_logged_out`) | AI aparece deshabilitada; no se piden modelos authed (`app/src/ai/llms.rs:1974-1977`) ni cuota (`app/src/ai/request_usage_model.rs:288-291`). El historial de conversaciones también se oculta (`app/src/settings/ai.rs:2283-2291`) |
| Enviar mensaje a Warp AI (logrado, server caído) | `app/src/ai_assistant/requests.rs:189-195` (`issue_request` → `generate_dialogue_answer`) | Error de red → mensaje "We're experiencing technical difficulties right now. Please try again later." en el transcript (`requests.rs:298-299`); sin retry |
| Settings > Teams | `app/src/settings_view/teams_page.rs:2516` (refresh al abrir) | `refresh_workspace_metadata` devuelve `Ok` vacío sin red para logged-out (`app/src/workspaces/update_manager.rs:159-163`). Qué widget se muestra: **no verificado** |
| Settings > Billing & Usage / Usage v2 | `app/src/settings_view/billing_and_usage_page.rs:785-789`, `billing_and_usage_page_v2.rs:2023-2025` | `refresh_request_usage` manda `None` sin red (`app/src/ai/request_usage_model.rs:288-291`); `usage_history_model.rs:54` salta fetch si `!is_logged_in()` |
| Settings > Referrals | `app/src/settings_view/referrals_page.rs:233` (`fetch_referral_status`), llamado en `:397`, `:412` | La llamada se hace al abrir la página; con server caído falla (manejo de error: **no verificado**); gate de login: **no encontrado** |
| Settings > Platform (API keys) | `app/src/settings_view/platform_page.rs:121` (`fetch_api_keys`), llamado en `:933` | Fetch al abrir; fallará con server caído (**manejo no verificado**) |
| Settings > Privacy (persistencia server-side) | `app/src/settings/privacy.rs:510-516`, `:544-550`, `:578-588` | Los toggles se guardan solo localmente si `!is_logged_in()`; no se hace red. Ojo: **si hay sesión persistida** sí intentará la llamada al server y fallará silenciosamente (callback `|_, _, _| ()`, `privacy.rs:512-515`) |
| Features login-gated genéricas | `app/src/auth/auth_manager.rs:730-743` (`attempt_login_gated_feature`), `:745-752` (límite de objetos Drive para anónimos) | Telemetría + modal de login (`AuthManagerEvent::AttemptedLoginGatedFeature`); suscrito en `app/src/workspace/view.rs:11778-11791` |

Botones "Sign in" adicionales: `app/src/workspace/view.rs:22443` (banner en workspace, p. ej. tras `DeniedAccessToken`).

---

## 4. Manejo de errores de red (toast / retry / ignora)

Patrón general: las llamadas van por `Result` (no `unwrap`/`expect` sobre respuestas de red en `app/src/server/server_api.rs`; los `.expect()` que existen ahí son sobre parses locales, p. ej. `server_api.rs:1416` parse de la URL base). Los reintentos usan `ctx.spawn_with_retry_on_error` con estrategias acotadas (§2).

1. **Refresh de usuario al arrancar (credenciales persistidas, server caído)**: `on_user_fetched` error → `AuthManagerEvent::AuthFailed(UserAuthenticationError::Unexpected)` (`app/src/auth/auth_manager.rs:585-598`) → `root_view.rs:3644-3666`: solo `report_error!` (Sentry). **No logout, no retry en bucle, no crash**: el usuario sigue en el workspace con su estado local (`is_logged_in` sigue true). `DeniedAccessToken` solo marca "needs reauth" (`auth_manager.rs:587-589`).
2. **Autoupdate**: fallo del check → `report_if_error!` (log/Sentry, `app/src/autoupdate/mod.rs:271-274`) — invisible para el usuario; el loop sigue cada 10 min (fallo repetido cada 10 min, no bucle denso). Si hubiera update con `update_by`, pide `server_time()` → `{server}/current_time` (`app/src/root_view.rs:2092-2096`; `app/src/server/server_api.rs:1364-1369`) que también fallará y se ignora.
3. **Poll de cloud objects / workspace metadata**: `spawn_with_retry_on_error` con 3 intentos (2 s, 4 s aprox) — `app/src/server/cloud_objects/update_manager.rs:699-719` (`PERIODIC_POLL_RETRY_STRATEGY`); al agotar, el próximo intento es el poll de 10 min. Sin toast. En cambio, **operaciones explícitas del usuario** (mover, borrar, restaurar, vaciar papelera) sí muestran **toast de error**: p. ej. `update_manager.rs:2127-2136` (MoveToFolder fallido → `ObjectOperationComplete { Failure }`), `:2182-2192`, `:4406`, `:4470`, `:4582`, `:4684`; y warnings "Not retrying" en `:2098`, `:2340`, `:2859`, `:4308`, `:4458`, `:4682`.
4. **Websocket RTC (listener)**: conexión rechazada → `LISTENER_RETRY_STRATEGY`: 5 intentos lineales de ~25 s (`app/src/server/cloud_objects/listener.rs:330-346` + `app/src/server/retry_strategies.rs:42-46`); mientras quedan reintentos → `log::warn` (`listener.rs:367-369`); agotados → `report_error!` y **se detiene** (`listener.rs:370-372`) — se reactiva solo por eventos (red online `:211-221`, CPU wake `:174-193`, cambio en CloudModel/UserWorkspaces). Reconexiones tras conexión exitosa esperan ≥30 s (`listener.rs:24`, `:359-365`). **No hay reconexión infinita incondicional.**
5. **Request de AI**: error de red → mensaje de error dentro del transcript ("We're experiencing technical difficulties…", `app/src/ai_assistant/requests.rs:298-299`) + telemetría del fallo; sin retry. Cuota: fallo de `get_request_limit_info` → `log::warn` (`app/src/ai/request_usage_model.rs:305-308`, `app/src/ai_assistant/requests.rs:163-165`); disponibilidad de créditos: `log::warn` y conserva last-known-good (`request_usage_model.rs:345-363`).
6. **Login manual fallido** (redirect/paste de token con server caído): notificación dismissable con texto + link a troubleshooting docs — `app/src/auth/login_failure_notification.rs:48-52` ("Request to log in failed.") y render `:70-76`; consumidores: `app/src/auth/login_slide.rs:423-441`, `app/src/auth/auth_view_modal.rs:291-318`, `app/src/auth/paste_auth_token_modal.rs:145-177`.
7. **Telemetría**: fallo de flush → `log::info!("Failed to flush events…")` y sigue (`app/src/auth/auth_manager.rs:559-564`); en shutdown espera máx 5 s para no colgar el quit (`app/src/server/telemetry/collector.rs:29-31`).
8. **Privacy settings server-side** (con sesión persistida): el callback descarta el error (`|_, _, _| ()`, `app/src/settings/privacy.rs:512-515`, `:546-549`, `:580-587`) — fallo totalmente silencioso.

Riesgos específicos buscados:
- **Reintentos infinitos: no encontrados** (todas las estrategias tienen `max retry count`: `app/src/server/retry_strategies.rs:24-46`, `:112`).
- **`unwrap()`/`expect()` sobre respuestas de red: no encontrados** en la ruta de requests. Los `.expect()` del código de server son sobre estado local (regex `app/src/server/cloud_objects/update_manager.rs:101`, objetos del modelo `update_manager.rs:3133`) o construcción de URLs (`app/src/server/server_api.rs:1416`, `:136`) — fallarían solo con config malformada, no por red caída.

---

## 5. Lista de prueba manual (server caído, login no obligatorio)

1. **Arranque en frío sin credenciales** (primera ejecución o tras logout): debe entrar a onboarding/workspace sin quedarse en spinner; verificar que no aparecen errores crudos.
2. **Arranque con credenciales persistidas**: debe abrir el workspace directo (estado local, `root_view.rs:1929`); esperar el fallo async del `refresh_user` (logs: `AuthFailed` / `report_error`); confirmar que NO te saca de la sesión ni crashea.
3. **Dejar la app abierta ≥15 min**: sin freeze/beach ball; en logs, cadencia de autoupdate cada 10 min y reintentos del listener 5×~25 s y luego silencio (no spam denso).
4. **Abrir Settings > Appearance** y cambiar tema: persistencia local debe funcionar sin red.
5. **Abrir Settings > AI**: página debe renderizar; con `is_anonymous_or_logged_out` AI está deshabilitada (`settings/ai.rs:2266-2275`); verificar que no cuelga el panel esperando modelos.
6. **Escribir `#` en el input del terminal / abrir Agent Mode**: con AI deshabilitada debe estar bloqueado o redirigir a login — no freeze ni petición colgada.
7. **Abrir Warp Drive** (panel izquierdo): verificar el botón "Sign in" para panels `RequiresAccount` (`left_panel.rs:317-331`) y que la lista local (sqlite) carga sin red.
8. **Intentar compartir un objeto del Drive** (menú contextual → Share): debe aparecer el modal de login (`drive/index.rs:5000-5009`), no un error crudo.
9. **Crear workflow/notebook desde Drive** con acciones gated: modal `RequireLoginCloseable` (`drive/index.rs:5338-5347`); sin gated, operación local + toast de fallo al intentar sincronizar (`update_manager.rs:2127-2136`).
10. **Abrir Settings > Teams, Billing, Referrals, Platform**: los fetch fallan o se saltan según login (`workspaces/update_manager.rs:159-163`, `request_usage_model.rs:288-291`, `referrals_page.rs:233`, `platform_page.rs:121`); comprobar que ninguna página queda en "loading" eterno.
11. **Settings > Privacy**: togglear telemetría/crash reporting — con sesión persistida intenta la llamada server-side y falla en silencio (`privacy.rs:510-516`); confirmar que el toggle local queda aplicado.
12. **Command Palette** (⌘P): buscar y ejecutar acciones locales (nueva tab, split, cambiar tema) — todo local, sin red.
13. **Toggle de red del sistema (offline → online)**: al volver online se rearrancan polls (`update_manager.rs:626-633`, `workspaces/update_manager.rs:81-97`) y reintentan acotadamente contra el server muerto; sin freeze.
14. **Flujo de login manual**: clic "Sign in" abre el navegador hacia la URL del server (`login_slide.rs` → `sign_in_url`, `auth_manager.rs:876-918` construye URLs con `server_root_url`): con `127.0.0.1:1` el navegador no cargará; confirmar que la app sigue usable y que pegar un redirect inválido solo muestra la notificación de fallo (`login_failure_notification.rs:70-76`).
15. **Onboarding con AI/Drive elegidos**: al terminar onboarding debe aparecer el login slide (por `requires_post_onboarding_login`, `root_view.rs:634-641`) y la opción de saltar login debe dejar entrar al workspace con AI/Drive deshabilitados (`drive/settings.rs:48-61`, `settings/ai.rs:2266-2275`).

---

### Pendientes / no encontrados
- `ChangelogModel` (`app/src/lib.rs:2144`): no se verificó si hace fetch de red al mostrarse.
- UI exacta de Teams/Billing/Referrals/Platform cuando el fetch falla con server caído (solo se verificó el gate y el punto de llamada).
- `referral.rs` / `integrations.rs` en `app/src/server/server_api/`: no inspeccionados por presupuesto (≤40 comandos de lectura).
