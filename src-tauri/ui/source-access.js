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
  const observed = !!facts?.continuous
  container.dataset.access = verifiedArchive ? (observed ? 'diagnostic_observations' : 'local_archive') : 'unverified'
  if (!verifiedArchive) {
    container.append(node('p', 'Для этого чата ещё не настроено чтение данных. Подключите поддерживаемый файл экспорта.', 'hint'))
    return
  }
  container.append(node('p', observed ? 'Начальная JSON-история + диагностические наблюдения Telegram · локально' : 'Локальный JSON · без входа в Telegram через TGSUM', 'source-method'))
  const details = document.createElement('details')
  details.className = 'source-access-details'
  details.append(node('summary', 'Что читает и сохраняет TGSUM'))
  const list = document.createElement('dl')
  const row = (name, text) => list.append(node('dt', name), node('dd', text))
  row('Чтение', observed ? 'Начальная история получена из JSON. Telegram пишет диагностические логи всех своих чатов; TGSUM читает выбранную папку и сохраняет наблюдения только подключённого чата. Логи не входят в результат.' : 'TGSUM читает выбранный JSON целиком. Другие чаты полного экспорта могут быть прочитаны при выборе и проверке файла.')
  const scope = facts.selected_scope
  row('В проекте', `Исходные сообщения выбранного чата · ID ${scope.conversation_id} · название аккаунта «${scope.account_local_id}». Вы задаёте название сами; оно помогает различать экспорты.`)
  row('В результате', 'Только выбранные темы, даты и сообщения. Настройки скрытия данных применяются при подготовке результата; локальная копия чата хранит исходные сообщения.')
  row('Авторизация', 'Для импорта файла не требуется. TGSUM не получает сессию или токен Telegram.')
  row('Обновление', observed ? (facts.continuous.enabled
    ? (facts.continuous.available ? 'Сбор включён в настройках проекта. Пока TGSUM запущен, журнал обновляет сохранённую историю. Фактическое состояние и ошибки показаны в разделе сборщика.' : 'Сбор сохранён в настройках, но на этой ОС он недоступен.')
    : 'Сбор остановлен. Сохранённые наблюдения остаются в истории и используются в локальном пакете. Возобновить сбор можно в разделе ниже.') : method.assisted_export
    ? 'Перечитать сохранённый файл или получить новый JSON в Telegram Desktop. TGSUM замечает возможные result.json только в назначенной папке; человек подтверждает окончание экспорта и чат перед импортом. Вход и экспорт выполняются в выбранном клиенте; TGSUM не читает его хранилище сессии. Автовыгрузка не включена.'
    : 'Перечитать сохранённый файл. Новые сообщения появятся после предоставления нового архива.')
  row('Вложения', `Импорт сохраняет ссылки на вложения. Файлы с компьютера нужно выбрать отдельно; сохранено выборов: ${facts.attachment_choices}. Их наличие и поддерживаемый формат проверяются при подготовке. Автоматического скачивания нет.`)
  row('Полнота', coverage
    ? (coverageNames[coverage.level] || 'Полнота истории не подтверждена.') + (coverage.known_gaps?.length ? ` Известных пропусков: ${coverage.known_gaps.length}.` : '')
    : 'Нет проверенных сведений о локальной копии. Импортируйте или восстановите архив.')
  row('Анализ с помощью ИИ', 'Импорт и подготовка выполняются на компьютере. Можно сохранить файлы в папку без ИИ. Для анализа выберите получателя, проверьте данные и отдельно нажмите «Начать анализ».')
  details.append(list)
  container.append(details)
}
