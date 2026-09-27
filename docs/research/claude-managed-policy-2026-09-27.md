# Claude Code 2.1.280: Linux managed policy перед передачей context

Дата: **2026-09-27**. Срез `tgsum-hzm.8`, продолжение
[auth/egress research](claude-auth-egress-2026-09-27.md).
Исследование выполнено без запуска CLI, чтения реальных credentials/settings
или обращения к аккаунтам. Это contract для реализации и синтетических тестов,
не runtime qualification.

## Источники и применимость

Первичный version-pinned источник — публичный native Linux x64 **2.1.280**:
[metadata](https://registry.npmjs.org/@anthropic-ai/claude-code-linux-x64/2.1.280),
[artifact](https://registry.npmjs.org/@anthropic-ai/claude-code-linux-x64/-/claude-code-linux-x64-2.1.280.tgz).
SHA-256 `1e08503dbdf3c2cb0d706d32f3408277388d1c76ef108673e8fe42c1b322925b`,
233 709 640 bytes. SDK **0.3.280**
[types and manifest](https://registry.npmjs.org/@anthropic-ai/claude-agent-sdk/-/claude-agent-sdk-0.3.280.tgz)
связывают его с buildDate `2026-09-21T20:55:27Z`, commit
`80abbfe7d7232280011ff01a21ae3338f4c6e372`.
Packages опубликованы 2026-09-22. Byte offsets далее относятся только к этому
SHA; прочитаны релевантные встроенные текстовые фрагменты, без исполнения.

Текущие official docs прочитаны 2026-09-27 и не закреплены за binary version.
Например, они уже описывают отдельные изменения 2.1.282. Такие изменения нельзя
приписывать 2.1.280. Кэш primary artifacts:
`/tmp/tgsum-claude-docs-20260927/`; это публичные package files, не профиль пользователя.

## 1. Какие источники нужно сохранить

Для **native Linux**, без WSL/MDM/gateway launcher, стандартный endpoint-managed
набор:

| Путь | Роль |
| --- | --- |
| `/etc/claude-code/managed-settings.json` | Основной managed settings document |
| `/etc/claude-code/managed-settings.d/*.json` | Дополнения к тому же file tier |
| `/etc/claude-code/managed-mcp.json` | Отдельный источник с exclusive MCP semantics |

Remote policy — отдельный, более высокий источник. `--setting-sources=`
не отключает managed tier. Private HOME сам по себе не переносит системные
файлы. WSL может наследовать Windows policy и требует отдельной квалификации.
[Managed delivery](https://code.claude.com/docs/en/managed-settings).

**Pinned details:** `SS/uf/Wvr` около byte 191305460 возвращают для Linux
`/etc/claude-code`; `Wvr()` в этом artifact пустой. Аналогично remote override
getter `RE()` в 191277322 пустой. Имена
`CLAUDE_CODE_MANAGED_SETTINGS_PATH` / `CLAUDE_CODE_REMOTE_SETTINGS_PATH`
есть в env registry и diagnostics, но это не доказательство работающего override.
Не использовать их как production contract. `CLAUDE_CONFIG_DIR` переносит
пользовательский config/cache, а не этот Linux system tier.
[Native 2.1.280](https://registry.npmjs.org/@anthropic-ai/claude-code-linux-x64/-/claude-code-linux-x64-2.1.280.tgz).

`managed-mcp.json` обнаруживается через тот же system directory (`hKt`,
197974364). Его наличие имеет смысл даже при пустом server set; нельзя просто
удалить файл, поскольку передан `--strict-mcp-config`. Managed servers могут
приходить также через `managedMcpServers` внутри settings.
[Managed MCP](https://code.claude.com/docs/en/mcp#managed-mcp-configuration),
[native artifact](https://registry.npmjs.org/@anthropic-ai/claude-code-linux-x64/-/claude-code-linux-x64-2.1.280.tgz).

### Точная загрузка file tier

В native `QEr` (191329571): base file первым, затем names из drop-in directory,
отфильтрованные по `endsWith(".json") && !startsWith(".")`, отсортированные
обычным JavaScript `Array.sort()`: порядок UTF-16 code units, без locale.
Native допускает regular files и symlink entries. Не-JSON и dotfiles игнорирует.

Settings reader `AOe` (около 191334390) использует strict `JSON.parse` через
`yt/se` (190869388), предварительно снимая один начальный BOM через `ns`
(190851130). Comments/trailing commas не поддерживаются этим reader, несмотря
на наличие другого JSONC helper в bundle. Пустой/whitespace файл даёт `{}`.
Native JSON parsing выбирает последнее повторное имя ключа; TGSUM может явно
отклонить дубликаты вместо неоднозначной собственной интерпретации.

File merge `vz` (191358675): объекты рекурсивно; массивы объединяются с
deduplication; scalar последнего файла побеждает. Исключения: `fallbackModel`
заменяется целиком, `modelPicker` целиком, одинаковые имена entries в
`extraKnownMarketplaces` / `managedMcpServers` заменяются целиком.
[Primary native artifact](https://registry.npmjs.org/@anthropic-ai/claude-code-linux-x64/-/claude-code-linux-x64-2.1.280.tgz).

**Предлагаемый capture:** сохранить exact bytes/names всех распознанных файлов
в bounded immutable snapshot и перенести read-only в private `/etc/claude-code`.
Проверять regular-file/ownership/size/count и стабильность directory inventory;
unreadable file, symlink, non-UTF-8 name или гонка — явная несовместимость,
а не «policy отсутствует». Не передавать весь host `/etc` и не редактировать
исходные файлы. Это ограничения TGSUM, более узкие, чем reader самого Claude.

## 2. Добавить force, сохранив admin documents

Remote → OS managed → file tier — приоритет admin sources. По умолчанию
выбирается первый с policy key; `managedSourcesBehavior:"merge"` меняет
композицию. Не устанавливать `merge` от имени TGSUM. `forceRemoteSettingsRefresh`
читается across admin sources: remote `false` не снимает file-tier `true`.
[Managed precedence](https://code.claude.com/docs/en/managed-settings#how-claude-code-combines-managed-sources).

Однако сначала весь file tier сворачивается: **поздний drop-in с `false`
может затереть более ранний `true`**. Getter `pkr` (191353289) видит уже
свёрнутые tiers. Ordinary `--settings` не заменяет managed force.
[Native 2.1.280](https://registry.npmjs.org/@anthropic-ai/claude-code-linux-x64/-/claude-code-linux-x64-2.1.280.tgz).

Практическое решение: оставить original base/drop-ins без изменений и добавить
**один собственный последний** drop-in `{"forceRemoteSettingsRefresh":true}`.
Проверять реальный порядок, не полагаться на имя `99-tgsum.json`. Например,
полное последнее имя плюс `.tgsum.json` строго больше своего prefix в JS order;
если имя не помещается в filesystem limit, отказать явно. Коллизии запрещены.
Own drop-in может только усиливать эту настройку; другие admin values он не меняет.

## 3. Самый простой preflight: тот же процесс, пустой context

Публичные SDK types **0.3.280** объявляют `get_settings` control request
(`sdk.d.ts:4045`) и NDJSON control envelope (`4774`, `4825`); native реализует
handler в 213030347. Установка Node SDK не нужна: runner может говорить этим
wire protocol напрямую. CLI reference документирует stream-json input/output.
[SDK package](https://registry.npmjs.org/@anthropic-ai/claude-agent-sdk/-/claude-agent-sdk-0.3.280.tgz),
[CLI flags](https://code.claude.com/docs/en/cli-reference).

Минимальное изменение уже ограниченного runner argv:

```text
-p --input-format stream-json --output-format stream-json --verbose
```

Сохранить существующие проверенные flags, private HOME, no-tools, credentials
capability, namespace и gateway. Prompt не передавать positional argument.
Открыть stdin, послать **только** первую control line:

```json
{"type":"control_request","request_id":"tgsum-init-1","request":{"subtype":"initialize"}}
```

Ожидаемый success envelope, с другими полями payload опущенными здесь:

```json
{"type":"control_response","response":{"subtype":"success","request_id":"tgsum-init-1","response":{"commands":[],"agents":[],"models":[],"account":{}},"pending_permission_requests":[],"pending_user_dialog_requests":[]}}
```

После correlated success:

```json
{"type":"control_request","request_id":"tgsum-settings-1","request":{"subtype":"get_settings"}}
```

Native response payload имеет `effective`, `sources:[{source,settings}]`,
`applied:{model,effort,advisor,ultracode}`, optional `errors` и
`remote_control_policy_lock_reason`. Значения ниже — схема примера, не ожидание
пустой policy у любого аккаунта:

```json
{"type":"control_response","response":{"subtype":"success","request_id":"tgsum-settings-1","response":{"effective":{},"sources":[],"applied":{"model":"REVIEWED_MODEL_ID","effort":null,"advisor":null,"ultracode":false}}}}
```

Только после успешной проверки отправить один user frame **в этот же процесс**:

```json
{"type":"user","message":{"role":"user","content":"REVIEWED_SANITIZED_CONTEXT"},"parent_tool_use_id":null,"client_composed":true}
```

Затем обычный bounded result protocol. Не выдавать control responses за results.
`client_composed:true` в pinned SDK types исключает slash-command dispatch,
`@path`/MCP mention expansion и обычный turn-start attachment pass для
подготовленного приложением context (`sdk.d.ts:6021`); это также проверить fixture.
Unknown request IDs, control error, deadline/size limit, pending permissions/
dialogs или неожиданный inference до user frame — отказ; context ещё не отправлен.
Не сохранять raw init/settings responses: они могут содержать email, org и env.
Shapes подтверждены `sdk.d.ts:314–358,4276–4400,4774–4830,5969` и native
`Uw/Oy` (213060767). Это proposal для canned tests, не выполненный тест.
[SDK/native package](https://registry.npmjs.org/@anthropic-ai/claude-agent-sdk/-/claude-agent-sdk-0.3.280.tgz).

### Действительно ли это после свежей remote policy?

**При действующем managed force — да по pinned startup order.** Commander
`preAction` (206715129) выполняет
`await dFt(() => wdt(...startupAwaited:true))`; ошибка завершает startup.
Только затем начинается print action и его control loop. `wdt`
(203816126) ждёт fetch/application. Это сильнее, чем сам `get_settings`,
который generic remote-fetch barrier не ожидает. Без force есть startup race.
С optional `--await-initialize` первая control line читается раньше, но reply
обрабатывается позже; этот флаг не обязателен, если не передаются plugins.
[Native artifact](https://registry.npmjs.org/@anthropic-ai/claude-code-linux-x64/-/claude-code-linux-x64-2.1.280.tgz).

Force гарантирует fetch только **eligible** profile. Ineligible route возвращает
успех без fetch (`qe`, 203810347). Это не свидетельство пустой server policy.
Также fresh response не обещает неизменность на весь Run: native поддерживает
последующие policy changes. OS/network ограничения должны оставаться внешними;
не выключать native updates ради «стабильного» снимка.

`resolveSettings()` SDK (`sdk.d.ts:3134`) — публичный alpha raw resolver без
запуска CLI, fresh fetch и исполнения `policyHelper`; его нельзя использовать
как live permission decision. Штатный `get_settings` в том же процессе проще.
Публичного CLI `settings` command или `--print-system-prompt`, решающего эту
задачу, в проверенных CLI reference/artifact не найдено.
[SDK types](https://registry.npmjs.org/@anthropic-ai/claude-agent-sdk/-/claude-agent-sdk-0.3.280.tgz).

## 4. Практический compatibility predicate

Сохранять original settings и позволять pinned CLI применять свою precedence.
Не реализовывать второй enterprise policy engine и не удалять несовместимые
ключи для успешного запуска. Для текущего runner без tools достаточно такого
**предлагаемого** gate:

| Проверка до user frame | Allow / отказ |
| --- | --- |
| Native/config identity | Проверенный binary pin, private config, original managed snapshot, own force; original source изменился — новый Review |
| Provider/receiver | Initialize account provider `firstParty`; gateway остаётся разрешать только reviewed destination. Другой provider, gateway login или endpoint/proxy/auth/header/CA overrides из policy несовместимы с этим профилем |
| Model | `applied.model` совпадает с явно reviewed model; иной model, advisor, ultracode или разрешённая policy fallback/substitution, не покрытая Review, требуют отказа/нового Review |
| Policy env | Проверить известные env override names без вывода значений. Минимальная реализация может отклонять любой непустой managed `env`; расширение только для явно проверенных нейтральных значений |
| Executable policy | Непустые `policyHelper`/`policyHelpers`, hooks, credential/proxy/telemetry helpers, commands/plugins/agent definitions — несовместимость до context, пока runner их не поддерживает |
| MCP/tools | CLI запускается с проверенным пустым tool set. Непустые `managedMcpServers`, server set в `managed-mcp.json` или иные активные MCP config несовместимы. Пустой original MCP file сохраняется |
| Errors/actions | Любой nonempty `get_settings.errors`, policy lock reason, pending permission/dialog, control error — отказ; отсутствие API request с corpus в canned peer |
| Other restrictions | Model allow/deny, permission restrictions, version/login constraints сохраняются и исполняются самим CLI; runner не ослабляет их. Противоречие выбранному execution profile даёт понятную техническую несовместимость |

Известные опасные features в **локальных original files** проверять до spawn:
helper/hook может сработать раньше initialize. Проверять `effective` вместе с
`sources` и original local snapshot: некоторые cross-source значения, в
частности env, не сводятся к простому «победил remote document».
Для remote policy preflight видит уже принятую native configuration;
изоляция credentials/files/network действует ещё до этого ответа.

Проверка effective state имеет точную границу: `fNn` (191691577) возвращает
parsed per-source settings, а `get_settings.errors` исключает warning severity.
Сам CLI может отбросить unknown/invalid entries с warning. Это не полный аудит
raw server document. Не утверждать обратного; применять contract к pinned CLI,
сохраняя original local bytes. Remote cache также не заменяет этот протокол:
`qe` (203812800+) может применить settings для noninteractive run без сохранения
их как consented disk cache. Эти факты подтверждены static native evidence.
[Native artifact](https://registry.npmjs.org/@anthropic-ai/claude-code-linux-x64/-/claude-code-linux-x64-2.1.280.tgz).

## 5. Personal subscription и организационные controls

В 2.1.280 eligibility проверяет stored subscription type: Team/Enterprise или
неизвестный `null` допускают remote fetch, Pro/Max — пропускают. Force это не
меняет. Initialize account data тоже локальная metadata (`$de`, 193371982),
а не свежая серверная аттестация плана. Поэтому нельзя читать `pro` как
доказательство отсутствия организации или переписывать его в `null` ради fetch.
[Eligibility evidence](claude-auth-egress-2026-09-27.md#проверка-идеи-с-постоянной-managed-policy-tgsum).

Практическое разделение capabilities:

- **Personal existing login:** перенос credential capability и endpoint files,
  native eligibility без изменений, тот же preflight, fixed receiver/model.
  Не требовать искусственно enterprise endpoint от каждого Pro/Max пользователя.
- **Fresh organizational policy:** claim возможен только после доказанного
  eligible fetch/application; local `pro/max` не выдаёт такой capability.
  Если конкретный профиль требует org identity/helper/launcher, которых private
  config не переносит, он явно несовместим, а не автоматически personal.
- **Metadata stale:** нет найденного публичного control запроса, который
  гарантированно обновляет subscription classification до startup eligibility.
  Не подменять авторизацию API key и не обещать решение stale membership этим
  preflight. Реальный переход plan/org и подтверждение профиля остаются в
  `tgsum-t8t.19` под контролем пользователя.

## Ближайшая проверяемая реализация

1. Bounded capture original Linux managed files + последний own force drop-in.
2. Stream-json init → get_settings → predicate → user в одном процессе.
3. Синтетические cases: поздний force false; BOM/JSONC; unreadable/symlink/
   replacement; compatible model deny; incompatible env/helper/MCP; delayed
   remote response; force failure; control timeout/malformed/unknown ID.
4. Peer обязан доказать **ноль corpus bytes до успешного preflight**, включая
   отказы. Последующие native output/result checks и namespace/egress остаются.

Этого достаточно для scoped code slice. Full enterprise compatibility,
неподдержанные managed features, реальные Pro/Max/Team переходы и account QA
не доказаны этим документом и остаются в указанном backlog.

## Реализованный synthetic runtime spike

После статического research реализованы capture/staging и same-process preflight
в `runner/src/claude/{policy,preflight}.rs`, подключённые к внутреннему HTTPS
профилю. Native **2.1.280** прошёл четыре TLS suites / **19 сценариев**, только
с вымышленным OAuth и локальным canned TLS peer. Host policy/auth не читались.

- Original main/fragments/MCP bytes сохраняются; отдельный последний fragment
  усиливает force, исходные файлы не меняются. Main/fragment minimum version
  блокируют launch; поздний force=false не отменяет startup barrier.
- Capture проверяет ownership/mode/тип/лимиты и exact bytes при launch. До spawn
  выполняется compatibility check локальных файлов: JSONC/duplicate keys,
  непустые helpers/hooks/MCP/plugins/env и fallback/receiver overrides отклоняются.
- `initialize → get_settings → user` выполняется в одном native процессе.
  Preflight подтверждает firstParty, reviewed model и проверяет effective/sources.
  Corpus удерживается relay, `/context` пустой. Control replies не выходят наружу.
- Remote env/fallback fixtures дают native policy fetch и exit 1, **0 Messages**.
  Success Pro и Team 200/204/404 проходят; version/403/304, wrong CA/receiver,
  expired, 401, cancellation/timeout сохраняют ожидаемое поведение.
- `client_composed:true`: `/help @/runtime/peer-address` остаётся в Messages
  буквальным текстом; содержимое runtime файла отсутствует.
- Unit fixtures проверяют подмену model/provider, pending actions, повторные
  keys/IDs, malformed/oversize/EOF и отсутствие corpus при rejected handshake.

Это доказательство ограниченного Linux профиля с synthetic inputs. Public cloud
runner, durable lifecycle и Desktop integration ещё не подключены. Реальные
plan/org transitions, refresh/revoke и enterprise/OS qualification остаются
`tgsum-t8t.19` (user control required); synthetic spike не закрывает их.

### Продолжение: библиотечный runner и durable completion

После preflight spike добавлены `ClaudeNetworkRunner`, `AnalysisJob` и
`RecipeRequest::job/run_recipe`. Те же 19 native synthetic TLS сценариев
теперь проверяют public qualification, execution с canned gateway и durable
completion. Успех сохраняет результат и коммитит baseline; ошибки/cancel не
меняют Project. Offline unit cases проверяют binding receiver/model/version/
profile/bundle/recipe, rejected evidence и stale Project: validated artifact
остаётся для явного recovery, baseline не продвигается. Desktop wiring и
реальные аккаунты по-прежнему не квалифицированы.
