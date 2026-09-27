# Claude Code: существующий OAuth login и egress

Дата: **2026-09-27**. Срез: `tgsum-hzm.8`, продолжение
[исследования CLI](claude-adapter-2026-09-27.md). Цель — использовать выбранный
существующий Linux subscription login, сохраняя ограниченный workspace и явного
получателя. API key не заменяет эту цель. Исследовательская часть выполнена без
запуска Claude; отдельное offline runtime доказательство добавлено ниже. Не читались
пользовательские auth/config/keychain, не выполнялись запросы к реальным
account/inference endpoints.

## Доказательства и применимость

Прочитаны текущие official docs, SDK **0.3.280**, а также релевантные текстовые
фрагменты, включённые в публично распространяемый native **2.1.280**. Это
статическое чтение артефакта, без его запуска и без исследования посторонних
компонентов. Веб-документы не закреплены за версией; поведение 2.1.280 ниже
отмечено отдельно. Публичная стабильная JSON Schema `.credentials.json` в
прочитанных auth docs и SDK types **не найдена**.

First-party artifact:
[npm metadata](https://registry.npmjs.org/@anthropic-ai/claude-code-linux-x64/2.1.280),
[tarball](https://registry.npmjs.org/@anthropic-ai/claude-code-linux-x64/-/claude-code-linux-x64-2.1.280.tgz).
`package/claude`: 233 709 640 bytes; SHA-256
`1e08503dbdf3c2cb0d706d32f3408277388d1c76ef108673e8fe42c1b322925b`.
Он совпал с `manifest.json` из
[SDK 0.3.280](https://registry.npmjs.org/@anthropic-ai/claude-agent-sdk/-/claude-agent-sdk-0.3.280.tgz):
buildDate `2026-09-21T20:55:27Z`, commit
`80abbfe7d7232280011ff01a21ae3338f4c6e372`.
Локальный исследовательский cache: `/tmp/tgsum-claude-docs-20260927/`;
скачанный `claude-2.1.280-public.bin` сохранён без executable permission.
Byte offsets ниже относятся только к этому SHA, не к произвольной версии CLI.

## Документированные границы auth/config

Linux login хранится в `~/.claude/.credentials.json` с mode 0600.
`CLAUDE_CONFIG_DIR` переносит этот файл, а также settings/history/plugins.
macOS Keychain identity тоже зависит от config directory. Subscription OAuth,
setup-token, Console API key и Console `user_oauth` profile — разные пути.
Setup-token предназначен для `CLAUDE_CODE_OAUTH_TOKEN`; Console profile может
жить в Anthropic config и иметь собственный refresh. В `-p` унаследованный
`ANTHROPIC_API_KEY` имеет приоритет над subscription login. Источник следует
выбирать явно, не принимать все auth environment одновременно.
[Authentication](https://code.claude.com/docs/en/authentication).

`--setting-sources=` отключает user/project/local settings, но не является
флагом отключения credentials. `--safe-mode` сохраняет auth, тогда как `--bare`
исключает OAuth/keychain. Вывод о совместимости пустого settings-source list
с выбранным credential file требует fixture-проверки конкретного binary.
[CLI reference](https://code.claude.com/docs/en/cli-reference),
[bare mode](https://code.claude.com/docs/en/headless#start-faster-with-bare-mode).

## Version-pinned наблюдения из артефакта

Следующее — implementation evidence, **не** обещание стабильного API.
Источник всех строк таблицы — public native 2.1.280 выше.

| Предмет | Наблюдение | Byte offset |
| --- | --- | --- |
| Reader | Читает `store.claudeAiOauth`; возвращает объект при непустом `accessToken` | 193348100–193349300 |
| Writer | Поля `accessToken`, `refreshToken`, `expiresAt`, `refreshTokenExpiresAt`, `scopes`, `subscriptionType`, `rateLimitTier`, `clientId` | 193344561, 193346561 |
| OAuth scope | Проверяет наличие `user:inference` | 190690793 |
| Expiry | Epoch milliseconds; refresh threshold `now + 300000 >= expiresAt` | 193289450, 193293443 |
| Token exchange/refresh | `POST https://platform.claude.com/v1/oauth/token`; JSON `grant_type`, `refresh_token`, `client_id`, `scope` для refresh | 190691619, 193288900 |
| Revocation | `POST https://platform.claude.com/v1/oauth/token/revoke`, refresh-token hint | 193291572 |
| Invalid refresh | Может очищать access/refresh и устанавливать expiry 0 через credential store | 193347596 |
| Profile | `GET https://api.anthropic.com/api/oauth/profile` с Bearer | 193285422 |
| Policy | `GET https://api.anthropic.com/api/claude_code/settings`, Bearer + `anthropic-beta`, `Cache-Control: no-cache` | 203799843, 203802000, 203806065 |
| Policy response | 200: `{uuid, checksum, settings}`; 204/404: нет policy; 304 требует прежнего checksum | 203798350, 203806184 |
| Mandatory refresh | Getter читает managed tiers; обычный flag settings в проверку не входит | 191353382 |

Credentials и refresh результат имеют разные структуры: token endpoint использует
snake_case, store — camelCase. Static evidence не доказывает, что успешный refresh
работает с read-only bind, что cleanup не пытается писать или что startup требует
только один endpoint. Эти вопросы проверяются отдельно.

## Сеть и CA: факты docs

Claude Code поддерживает `HTTPS_PROXY`/`HTTP_PROXY`; lowercase варианты имеют
приоритет, `NO_PROXY` способен исключить запросы. SOCKS не поддержан.
Native runtime поддерживает bundled и system CA; `CLAUDE_CODE_CERT_STORE`
выбирает источники (`bundled,system` по умолчанию), `NODE_EXTRA_CA_CERTS`
добавляет PEM. Отключение TLS validation не требуется. Документация включает
native runtime, но прохождение Bun 1.4.3 через TGSUM CONNECT/SNI relay ещё
надо доказать. Публичный CA-файл не равен mTLS client private key.
[Network configuration](https://code.claude.com/docs/en/network-config).

Inference, policy, profile и часть telemetry разделяют `api.anthropic.com`.
`platform.claude.com` обслуживает OAuth exchange/refresh/revoke. Поэтому host
allowlist не различает inference и metadata на одном host. Env отключения
неосновного трафика снижает побочные обращения, но не заменяет gateway и не
даёт обещания «на host идут только Messages».
[Network destinations](https://code.claude.com/docs/en/network-config#network-access-requirements).

## Managed policy — обязательная граница

`--setting-sources=` не отключает managed settings. Linux system policy находится
в `/etc/claude-code/managed-settings.json`, с возможными `managed-settings.d/*.json`
и отдельным `managed-mcp.json`. SDK parent settings не заменяют admin policy.
Флаг `--settings '{"forceRemoteSettingsRefresh":true}'` **не подтверждает**
mandatory fetch: настройка читается из managed tier. SDK types описывают её
именно в этом scope; статический getter 2.1.280 согласуется с docs.
[Managed delivery](https://code.claude.com/docs/en/managed-settings),
[SDK 0.3.280, Settings](https://registry.npmjs.org/@anthropic-ai/claude-agent-sdk/-/claude-agent-sdk-0.3.280.tgz).

Server policy требует direct Anthropic endpoint и eligible credential;
Team/Enterprise OAuth входит в список. Non-default `ANTHROPIC_BASE_URL`
пропускает fetch, поэтому такой mock не доказывает production policy flow.
Без cache startup может продолжиться до окончания fetch; ошибка fetch по
умолчанию не блокирует запуск. Cache — `~/.claude/remote-settings.json`.
`forceRemoteSettingsRefresh:true` из managed source заставляет ждать свежую
policy и завершаться при ошибке; ключ не создаёт eligibility там, где fetch
вообще выключен. Server response с командами/headers может менять полномочия
запуска, а managed hooks переживают обычное отключение hooks.
[Server-managed settings](https://code.claude.com/docs/en/server-managed-settings),
[Hooks](https://code.claude.com/docs/en/hooks#disable-or-remove-hooks).

**Вывод для TGSUM:** private config + один read-only credential file сами по себе
не сохраняют policy пользователя/организации. Разрешение hostname также не
доказывает загрузку policy до inference. Нельзя считать значение `subscriptionType`
в локальном JSON достаточной проверкой отсутствия organizational policy.
До квалификации необходимо явно сохранять эту capability неподтверждённой;
не маскировать её переходом на API-key-only.

### Проверка идеи с постоянной managed policy TGSUM

Предложение: read-only `/etc/claude-code/managed-settings.json` со значением
`forceRemoteSettingsRefresh:true` плюс gateway к `api.anthropic.com`.
Это настоящий Linux admin-file tier; `true` является cross-source lock:
обычный `--settings` или remote `false` не должны снять требование. Однако оно
действует только при **eligible fetch**. В pinned 2.1.280 eligibility
(offset 194338804) проверяет OAuth и subscription type: `team`/`enterprise`
либо неизвестный `null` проходят; `pro`/`max` сами по себе — нет.
Getter проверяет access token, а не `user:profile`; наличие этого scope для
успешной серверной авторизации endpoint данным чтением не установлено.
[Managed cross-source rules](https://code.claude.com/docs/en/managed-settings#keys-read-from-every-admin-source),
[native artifact 2.1.280](https://registry.npmjs.org/@anthropic-ai/claude-code-linux-x64/-/claude-code-linux-x64-2.1.280.tgz).

Для fixture с Team/Enterprise ожидаемая таблица при действующем force:

| Ответ settings endpoint | Ожидание, которое нужно проверить runtime |
| --- | --- |
| 200, валидные settings | Применение restrictions до первого Messages |
| 204 или 404 | Успешный ответ «policy нет», остаётся file policy |
| 403 | Ошибка startup, inference не начинается |
| 401 | Возможна попытка token recovery; при запрещённом refresh — ошибка |
| TLS/network failure/timeout | Ошибка startup после ограниченного ожидания |
| 304 без прежнего checksum | Ошибка; нельзя принять как пустую policy |

Статический fetch-код (offset 203806184–203808900) различает эти ответы;
это не засчитанная проверка отказа до inference. Постоянный файл TGSUM полезен
как **добавочное ограничение**, но не как замена исходной policy: изоляция может
скрыть host `managed-settings.d`, `managed-mcp.json`, login restrictions,
corporate launcher или `policyHelper`. Сохранение их смысла требует отдельного
контракта и проверки совместимости. Если helper/hook невозможно выполнить
в ограниченном runtime, требование не удаляется ради успешного Run.

## Предлагаемый синтетический контракт

Это test data, не инструкция изменять login пользователя. Начальная fixture,
выведенная из pinned reader/writer, подлежит проверке до признания минимальной:

```json
{
  "claudeAiOauth": {
    "accessToken": "tgsum-synthetic-oauth-access",
    "refreshToken": "tgsum-synthetic-oauth-refresh",
    "expiresAt": 0,
    "scopes": ["user:inference"],
    "subscriptionType": "pro",
    "rateLimitTier": null
  }
}
```

Fixture builder заменяет `expiresAt` на `now_ms + 86400000` для fresh,
`now_ms - 1` для expired; отдельные варианты — near-expiry, пустой refresh,
malformed JSON и Team/Enterprise. Fresh test не передаёт API key/OAuth-token env.
Selected-file capability остаётся opaque: exact descriptor read-only bind в
private config; исходный файл и его parent directory недоступны для записи.

| Проверка | Требуемое доказательство |
| --- | --- |
| Fresh subscription | Canned TLS peer принимает Bearer fixture, штатный Messages result; API-key fallback отсутствует |
| Config isolation | `CLAUDE_CONFIG_DIR` + `--setting-sources=` находят только выбранный файл, игнорируют соседние hooks/MCP/settings |
| TLS routing | Настоящие hostnames/SAN, явный test CA, CONNECT/SNI allowlist; без production base URL override |
| Expiry/401 | Нет результата при запрещённом refresh; bounded completion, неизменность host credential, последующий Run требует нового Review |
| Refresh ownership | Если refresh разрешён: rotation/write/conflict отдельно доказаны; immutable copied token не объявляется working refresh chain |
| Policy | Fresh/cache/failure/204/404/304, enforced model deny до Messages; fake server policy нельзя игнорировать ради зелёного теста |
| Leakage | Access/refresh и config text отсутствуют в UI, сохранённых diagnostics, stdout/stderr; отсутствует неожиданный egress |

Для первого fresh-token spike можно ограничить gateway `api.anthropic.com:443`,
а refresh host явно отклонять. Это **временный проверяемый режим**, не полная
поддержка existing login. Он должен давать техническую причину повторного входа
при expiry, а не сам выполнять login/logout/revoke. Отдельный synthetic peer для
`platform.claude.com` нужен, чтобы установить поведение refresh без реального
аккаунта; расширять production allowlist по одним строкам binary нельзя.

## Открытая квалификация и backlog

Код и canned tests могут двигаться независимо: минимум OAuth fixture, фактический
proxy/CA handshake, refresh failure, сохранение policy и базового result pipeline.
Для release нужны доказательства endpoint-managed policy inheritance и того,
что server policy применяется до передачи context. Если policy требует host
helper/hook/keyring/mTLS, ограниченная оболочка должна сообщать несовместимость,
а не скрыто исключать требование.

В `tgsum-t8t.19` под контролем пользователя остаются реальный subscription login,
expiry/renewal/revoke, Team/Enterprise managed environment и ОС-specific stores.
Наблюдения этого документа не закрывают эти проверки и не повышают support.

## Отдельная offline проверка выбранного OAuth-файла

После исследования `runner/tests/claude.rs` проверил native **2.1.280** в private
network/PID namespace. Синтетический `.credentials.json` с future expiry, scope
`user:inference` и subscription `pro` смонтирован read-only. API-key и token env
не передавались. Canned Messages server получил Bearer sentinel из этого файла;
typed Summary result прошёл evidence validator. Исходный файл и замена по старому
пути не изменились; соседние config files не попали в private HOME. Access и
refresh sentinels отсутствовали в stdout/stderr. Отдельная fake fixture проверяет
read-only mount, отсутствие mount FD в payload и отказ при неверном profile.

**Область доказательства:** чтение выбранного файла, bearer и protocol. Mock
задаёт `ANTHROPIC_BASE_URL` на private HTTP loopback, поэтому не проверяет
HTTPS/proxy/default-host policy eligibility/refresh/реальную подписку. Для этих
свойств остаются проверки из таблицы выше; облачная поддержка не объявлена.

## Следующий срез: native default-hostname HTTPS

`runner/src/claude/network_tests.rs` и `runner/tests/support/claude_https.rs`
проверяют production relay с native **2.1.280**, выбранным synthetic auth и
локальным TLS peer. SAN равен `api.anthropic.com`; используется только явный
test CA через `NODE_EXTRA_CA_CERTS`. `ANTHROPIC_BASE_URL`/API key/token env
отсутствуют. DNS override существует только у `#[cfg(test)]` gateway. Host loopback
недоступен payload напрямую; auth и mount FD проверки проходят.

| Fixture | Наблюдение native CLI |
| --- | --- |
| Fresh Pro | Один Messages, validated structured result, clean gateway |
| Team settings 200 | Settings → policy_limits → Messages, result принят |
| Team settings 204/404 | Такой же порядок; policy отсутствует, result принят |
| Settings 403/304 без cache | Exit 1, ни одного Messages |
| Settings 200, requiredMinimumVersion 99.0.0 | Exit 1 до Messages: remote constraint применён |
| Wrong CA / receiver | Запуск завершается ошибкой, inference peer не получает HTTP Messages |
| Messages 401 | Один Messages, exit 1 |
| Expired token | Два запрещённых CONNECT, затем один canned Messages и exit 0; TGSUM отклоняет по gateway report |
| Cancel / timeout | Соответствующая termination, gateway/namespace очищены |

Это **13 сценариев в трёх ignored suites**, запущенных явно. Ошибка forbidden
refresh имеет приоритет над успешным JSON/exit; это проверено через
`NetworkOutput::decode`. Refresh/revoke destination остаётся запрещённым, host
auth не меняется, sentinel credentials отсутствуют в stdout/stderr.

Профиль добавляет read-only `forceRemoteSettingsRefresh:true` в Linux managed
file. Проверки выше доказывают применимость этого ограничения к synthetic Team
startup; исходная host policy и настоящая организационная среда не переносились.
Public cloud runner/lifecycle/UI пока не добавлены. В частности, policy helpers,
managed hooks, локальные admin restrictions и замена модели требуют собственного
контракта до Run. Описанная в docs
[область model restrictions](https://code.claude.com/docs/en/model-config#restrict-model-selection)
не заменяет native квалификацию именно выбранной версии.

### Два уточнения по публичному артефакту и runtime

- Обнаружен `GET /api/claude_code/policy_limits`, отдельно от `/settings`.
  В том же публичном SHA путь находится по offset **191932502**; parser около
  **193693343** принимает `restrictions`, `compliance_taints`, `monitoring_notice`,
  `defaults`. Canned peer возвращает пустые ограничения. Нельзя обобщать эту
  fixture на все организационные правила.
- Schema comment около **192440274** описывает `product_feedback_disabled` как
  состояние организационного `allow_product_feedback`. При пустых restrictions
  native init содержит `false`, хотя analytics disabled и nonessential traffic
  выключен. Поле не означает факт отправки feedback. Decoder теперь требует
  boolean вместо обязательного true; env/tool/slash restrictions сохранены.
  Отдельный regression-тест падал до исправления.

Первые эксперименты не засчитаны: неполный runtime manifest не прошёл version
probe; первая Team fixture не знала `/policy_limits`; затем обнаружилось неверное
предположение decoder о feedback metadata. После исправления причин все сценарии
повторены. Реальных аккаунтов и обращений к провайдеру не было.
