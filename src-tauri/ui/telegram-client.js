export function telegramClientText(view) {
  if (view?.phase === 'failed') return `Режим Telegram требует внимания: ${view.message || 'повторите запуск или восстановление.'}`
  if (view?.phase === 'managed_debug') return `TGSUM включил отладку Telegram. Чатов с управляемым сбором: ${view.enabled_sources}. Прежний режим вернётся после остановки последнего из них или завершения TGSUM.`
  if (view?.phase === 'user_debug') return 'Отладка Telegram уже была включена вами. TGSUM оставит её включённой после остановки.'
  return 'TGSUM сейчас не управляет режимом Telegram.'
}
