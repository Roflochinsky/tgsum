// Access facts describe the implemented import operation. Selection is not an
// authorization grant, and snapshot coverage is not a connector-wide promise.
const coverageNames = {
  complete: 'Полнота подтверждена для диапазона архива.',
  partial: 'История неполная.', own_messages_only: 'Есть только сообщения владельца архива.',
  future_only: 'Есть только события после подключения.', unknown: 'Полнота истории не подтверждена.',
}

export function renderSourceAccess(container, facts, coverage) {
  const node = (tag, text, className) => {
    const element = document.createElement(tag)
    element.textContent = text
    if (className) element.className = className
    return element
  }
  container.replaceChildren()
  const method = facts?.method
  const verifiedArchive = method?.kind === 'archive' && method.credentials === 'none' && method.refresh === 'reimport'
  container.dataset.access = verifiedArchive ? 'local_archive' : 'unverified'
  if (!verifiedArchive) {
    container.append(node('p', 'Способ получения данных не подключён. Права доступа не подтверждены; выбор чата сам по себе их не ограничивает.', 'hint'))
    return
  }
  container.append(node('p', 'Локальный JSON · без входа в Telegram через TGSUM', 'source-method'))
  const details = document.createElement('details')
  details.className = 'source-access-details'
  details.append(node('summary', 'Доступ и данные источника'))
  const list = document.createElement('dl')
  const row = (name, text) => list.append(node('dt', name), node('dd', text))
  row('Чтение', 'TGSUM читает выбранный JSON целиком. Другие чаты полного экспорта могут быть прочитаны при выборе и проверке файла.')
  const scope = facts.selected_scope
  row('В проекте', `Исходные сообщения выбранного чата · ID ${scope.conversation_id} · локальная метка аккаунта «${scope.account_local_id}». Метка не подтверждает владельца аккаунта.`)
  row('В контексте анализа', 'Только выбранные темы, даты и сообщения. Очистка применяется при подготовке контекста; локальная копия чата хранит исходные сообщения.')
  row('Авторизация', 'Для импорта файла не требуется. TGSUM не получает сессию или токен Telegram.')
  row('Обновление', method.assisted_export
    ? 'Перечитать сохранённый файл или получить новый JSON в Telegram Desktop. TGSUM замечает возможные result.json только в назначенной папке; человек подтверждает окончание экспорта и чат перед импортом. Вход и экспорт выполняются в выбранном клиенте; TGSUM не читает его хранилище сессии. Автовыгрузка не включена.'
    : 'Перечитать сохранённый файл. Новые сообщения появятся после предоставления нового архива.')
  row('Вложения', `Импорт сохраняет ссылки на вложения. Для контекста отдельно выбираются локальные файлы; сохранено выборов: ${facts.attachment_choices}. Их наличие и допустимость проверяются при подготовке. Автоматического скачивания нет.`)
  row('Полнота', coverage
    ? (coverageNames[coverage.level] || 'Полнота истории не подтверждена.') + (coverage.known_gaps?.length ? ` Известных пропусков: ${coverage.known_gaps.length}.` : '')
    : 'Нет проверенных сведений о локальной копии. Импортируйте или восстановите архив.')
  row('Передача AI', 'Импорт и подготовка выполняются локально. Получатель выбирается на шаге анализа и показывается перед отдельной кнопкой Run. Export only сохраняет контекст в локальную папку.')
  details.append(list)
  container.append(details)
}
