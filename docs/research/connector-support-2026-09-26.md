# Поддержка источников: проверка API и экспортов

Проверено **2026-09-26** по публичным первичным источникам. Это первичный
research review: тестовые аккаунты, реальные fixtures, регистрация приложений
и допустимость конкретного договора с AI-провайдером ещё не проверены.
`candidate` означает перспективный следующий источник, а не выпущенную функцию.
Текущий TGSUM поддерживает только полный JSON-экспорт Telegram Desktop.

## Решение по очередности

| Приоритет | Источник | Статус / условие следующего шага |
| --- | --- | --- |
| Сейчас | Telegram full export | Сохранить существующий локальный импорт |
| P0 | Telegram single-chat export + повторный импорт | Устранить несовместимость формы JSON; проверять готовность snapshot |
| P1 import | WhatsApp export, Slack export | Fixtures, границы покрытия, provenance; никакого входа в личный WhatsApp |
| P1 import | Yandex archive | Получить реальный разрешённый fixture и подтвердить формат/состав |
| P1 conditional | Google Chat Takeout | Поддерживать фактически доступные архивы; корпоративный scope не обещать |
| P2 import | LINE export | Локали, многострочный текст, неполная история и отсутствие native IDs |
| P1 live research | Yandex Bot | Polling без публичного сервера; проверить хранение событий и AI-use |
| P1 live research | Google Chat user OAuth | История + space events; проверить retention, consent и разрешения приложения |
| P1 live research | Teams Graph | Отдельно delegated и RSC; tenant/admin и доставка уведомлений |
| P1 live research | Slack App | Развести internal Socket Mode и коммерческую дистрибуцию |
| P1 live research | MAX Bot | Admin access, токен, новые endpoint/сертификаты; отдельный AI-policy review |
| P2 research | VK, Discord Bot, Signal local | Нельзя обещать поддержку до закрытия перечисленных ниже неизвестных |
| Defer | Viber | Backup/restore и бот не решают универсальный импорт личной истории |

Все AI/cloud пути требуют отдельного review. «Официальный API» и «архив»
описывают получение данных, но не разрешение на произвольное downstream использование.
Точный статус каждого способа — [registry](../connectors/registry.json).

## РФ

### MAX

Open Client API требует участия в программе; полная документация выдаётся
участникам. Публичные правила ограничивают архитектуру прямым соединением
устройства с MAX и запрещают массовую выгрузку, индексирование, длительное
хранение, объединение с другими источниками и передачу данных в ML-системы.
Для проектируемого corpus этот путь **исключён из поддержки**.
[Open Client API](https://legal.max.ru/openclient-docs).

Bot API `GET /messages` документирует историю чата/канала при admin membership
бота, обратный порядок сообщений и `count` до 100. В текущей документации
предписан `platform-api2.max.ru`, заголовок Authorization и добавление сертификата
Минцифры в доверенные. Описания `from`/`to` нетипичны: `from` указан как верхняя,
`to` как нижняя временная граница, обе в миллисекундах. Их нельзя переносить
в код по названию: нужен контрактный тест границ и сообщений с одинаковым временем.
Admin credential не равен read-only credential. Токен может быть отозван.
[GET messages](https://dev.max.ru/docs-api/methods/GET/messages).

Доступность метода не доказывает разрешённость multi-source AI workspace.
Нужна проверка правил конкретного бота, требования к регистрации, trust store
в поддерживаемых ОС, updates/gap recovery и объёма разрешённого хранения.
[Правила платформы](https://dev.max.ru/docs/legal/rules).
Публичный пригодный archive format в этой проверке не подтверждён.

### Яндекс Мессенджер

Официальная справка описывает заказ архива через Управление данными → Мессенджер;
архив защищён паролем. Это подтверждает наличие пользовательского пути, но
не точную структуру, корпоративную доступность и полноту всех участников.
[Управление данными](https://m.yandex.ru/support/yandex-360/customers/messenger/ru/data).

Bot API относится к Яндекс 360 для бизнеса. `getUpdates` возвращает доставленные
боту сообщения чатов, где он подписчик, участник или администратор. Следующий
`offset` удаляет более старые updates из очереди: checkpoint сохранять только
после локальной фиксации обработанной пачки. Один токен не должен иметь несколько
независимых потребителей, которые сдвигают общую очередь.
[Обзор](https://yandex.ru/dev/messenger/doc/ru/),
[Polling](https://yandex.ru/dev/messenger/doc/ru/api-requests/update-polling).

Произвольный backfill старой истории через изученный polling-метод не подтверждён.
Связка archive + bot возможна только после проверки совместимости chat/message IDs,
момента начала подписки, overlap, потерь при простое и условий AI/retention.
Присутствие бота сообщает о нём участникам, но не заменяет нужные согласия.

### VK

Официальный SDK содержит `getHistory(UserActor)` и `getLongPollHistory(UserActor)`.
Наличие API-метода не доказывает, что новый распространяемый TGSUM получит нужные
user scopes. Страница `dev.vk.com/ru/method/messages.getHistory` не открылась
инструментом исследования. Выдача доступа, актуальная регистрация, downstream
AI-use и санкции остаются **неподтверждёнными**. Статус — research.
[VKCOM SDK](https://github.com/VKCOM/vk-java-sdk/blob/master/sdk/src/main/java/com/vk/api/sdk/actions/Messages.java).

## Международные платформы

### Teams

Graph поддерживает подписку на сообщения конкретного чата. Однако delegated
`Chat.Read` даёт чтение чатов от имени пользователя; выбор resource в подписке
сам по себе не сужает credential до этого resource. Для application permissions
документирован `ChatMessage.Read.Chat` через RSC, но именно страница notifications
помечает этот вариант как beta. Это требует отдельной проверки version/channel
перед обещанием scoped production connector.
[Notifications](https://learn.microsoft.com/en-us/graph/teams-changenotifications-chatmessage),
[Permissions](https://learn.microsoft.com/en-us/graph/permissions-reference#chatread).

Export API — отдельный административный сценарий с application permissions и
требованиями к Teams license. Он не тождественен обычному пользовательскому
«скачать архив». Надо также решить доступность webhook, продление subscription,
пропущенные изменения, доступ к файлам и корпоративные правила AI.
[Export APIs](https://learn.microsoft.com/en-us/microsoftteams/export-teams-content).

### Google Chat

Takeout содержит сообщения/вложения, но справка исключает некоторые group messages
и spaces, созданные work/school пользователями. Для организации может понадобиться
администратор; внешний domain также влияет на полноту. Это существенное ограничение
для заявленного B2B ICP.
[Export Chat data](https://support.google.com/chat/answer/10126829?hl=en).

User OAuth позволяет читать сообщения space с `chat.messages.readonly`; это
не доказательство token grant только на выбранные spaces. `spaceEvents.list`
даёт события за последние 28 дней и актуальную версию затронутого ресурса,
а не неограниченный журнал всех ревизий. Подходит для исследования локального
polling; нужен full reconciliation после длинного простоя.
[List messages](https://developers.google.com/workspace/chat/list-messages),
[List space events](https://developers.google.com/workspace/chat/list-space-events).

Workspace policy допускает определённые productivity/reporting use cases,
но ограничивает передачу, обучение моделей и постоянные копии/сроки cache.
Значит постоянный локальный corpus и конкретный AI handoff надо проверить
отдельно; наличие OAuth не делает эту архитектуру автоматически допустимой.
[Workspace data policy, Limited Use](https://developers.google.com/workspace/workspace-api-user-data-developer-policy).

### Slack

Экспорт — ZIP с JSON и ссылками на файлы; он не гарантирует локальное наличие
всех attachment bytes. Private/DM export зависит от плана и одобрения. В части
планов/одобренных export flows есть recurring exports — изучить официальный
механизм раньше UI-автоматизации браузера.
[Формат](https://slack.com/help/articles/220556107-How-to-read-Slack-data-exports),
[Доступ и scheduled exports](https://slack.com/help/articles/201658943-Export-your-workspace-data).

`conversations.history` ограничен для определённых коммерческих non-Marketplace
приложений до 1 запроса/мин и 15 сообщений. Применимость зависит от типа приложения
и установки. User token потенциально видит больше каналов, чем выбранный scope.
[Метод и лимиты](https://docs.slack.dev/reference/methods/conversations.history/).

Socket Mode даёт events по исходящему WebSocket без публичного HTTP endpoint,
но документация исключает такие приложения из публичного Marketplace.
Internal customer-owned app и распространяемый TGSUM требуют разных профилей.
Events не отменяют bootstrap, reconnect gaps и reconciliation.
[Socket Mode](https://docs.slack.dev/apis/events-api/using-socket-mode/).
Проверка AI-use/retention должна включить применимые API Terms и связанные
политики, а не только страницу метода.
[API Terms](https://slack.com/terms-of-service/api).

### WhatsApp Personal

Поддерживать предоставленный пользователем per-chat text export, с отдельной
проверкой iOS/Android, локали, вложений и границ доступной истории. Официальная
справка подтверждает export с медиа или без них и текстовый формат.
[Export chat](https://faq.whatsapp.com/1180414079177245/?cms_platform=android&locale=lt_LT).
Web scraping, извлечение сессий и автоматизацию личного клиента не включаем
в архитектуру продукта. Это граница выбранной поддержки; точные санкции здесь
не ранжируем без отдельной актуальной проверки. Business Platform — другой
use case, в текущую очередь не входит.

### Discord

Data Package содержит отправленные самим пользователем сообщения; полноценный
транскрипт всех участников из него обещать нельзя. Self-bots официально запрещены.
[Data Package](https://support.discord.com/hc/en-us/articles/360004957991-Your-Discord-Data-Package),
[Self-bots](https://support.discord.com/hc/en-us/articles/115002192352-Automated-User-Accounts-Self-Bots).

Bot integration — отдельный research: permissions, message-content intent,
история/события и условия данных. Developer Policy запрещает scraping и обучение
AI на message content без разрешения. Из запрета training нельзя вывести ни
общий запрет, ни общее разрешение inference; остальные условия продолжают действовать.
[Developer Policy](https://support-dev.discord.com/hc/en-us/articles/8563934450327-Discord-Developer-Policy).

### Signal, LINE, Viber

- **Signal:** официальная справка признаёт локальное хранение и community
  parsers. Это не гарантирует универсальную незашифрованную БД или стабильный
  формат всех ОС. Исследовать конкретную предоставленную backup/copy, необходимые
  пользовательские ключи, consistency и лицензию parser. Сеть/linked-device bot
  не нужны для выбранного local importer.
  [Signal export](https://support.signal.org/hc/en-us/articles/360007059412-Signal-and-the-General-Data-Protection-Regulation-GDPR).
- **LINE:** официальный text export есть; desktop сохраняет доступные в чате
  сообщения. Нужны fixtures и честная маркировка неполноты.
  [LINE Help](https://help.line.me/line/smartphone?contentId=20007388&lang=en).
- **Viber:** Bot API требует токен и webhook; новые боты создаются на коммерческих
  условиях. Это не API чтения всей личной истории. Пригодный архив для нашего
  importer в этой проверке не подтверждён, поэтому defer.
  [Viber REST API](https://developers.viber.com/docs/api/rest-bot-api/).

## Что поменялось относительно исходного предложения

Убираем обещания «нет риска бана», «полный архив на любой платформе» и «токен
видит только выбранные чаты» без подтверждения. Разделяем documented capability,
проверенный fixture/контракт и право на конкретное дальнейшее использование.
Приоритеты показывают порядок исследования и реализации, а не готовые релизы.
