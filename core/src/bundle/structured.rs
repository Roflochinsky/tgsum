//! Structured handoff uses exactly the saved selection and privacy pipeline.
//! Native IDs, paths, raw revisions and the reverse index stay in the Project.
use super::*;

impl ProjectStore {
    pub(crate) fn structured_package_files(
        &self,
        project_id: &str,
        bundle_id: &str,
        destination: &Path,
        cancelled: &impl Fn() -> bool,
    ) -> io::Result<Vec<BundleFile>> {
        let (_, private, manifest) = self.checked_bundle(project_id, bundle_id, cancelled)?;
        let project = self.read_revision(project_id, private.project_revision)?;
        let mut scan = ScanReview::new(project.privacy_options.clone(), self, &project)?;
        scan.discover_participants(self, &project, cancelled)?;
        let key = EvidenceKey::load(&self.directory(project_id)?)?;
        let snapshots = self.snapshots(project_id)?;
        let path = destination.join("messages.jsonl");
        let mut output = BufWriter::new(files::create_private(&path)?);
        let mut count = 0;
        for input in &private.inputs {
            check_cancel(cancelled)?;
            let snapshot = snapshots.load(&input.snapshot_id)?;
            if snapshot.source != input.source {
                return Err(invalid("structured source mismatch"));
            }
            let selected = select_messages(&snapshot, &input.selection, None)?;
            if input.selection.only_changes {
                return Err(invalid(
                    "structured handoff requires the selected full period",
                ));
            }
            let source = key.opaque("source", &input.source)?;
            let refs: BTreeMap<_, _> = selected
                .messages
                .iter()
                .map(|m| Ok((m.key.message_id.clone(), key.reference(m)?)))
                .collect::<io::Result<_>>()?;
            for message in selected.messages {
                check_cancel(cancelled)?;
                let evidence = &refs[&message.key.message_id];
                let text =
                    scan.clean(&message.text, "text", Some(evidence), Some(&input.source))?;
                let sender = scan.clean_sender(message, evidence)?;
                let timestamp = scan.clean(
                    message.timestamp.as_deref().unwrap_or("Unknown date"),
                    "timestamp",
                    Some(evidence),
                    Some(&input.source),
                )?;
                // Topic creation is its own root even though the normalized
                // service marker has no parent thread, matching scope filters.
                let topic_id = if message.service_action.as_deref() == Some("topic_created") {
                    Some(&message.key.message_id)
                } else {
                    message.thread_id.as_ref()
                };
                let topic = topic_id
                    .map(|id| key.opaque("thread", &(&input.source, id)))
                    .transpose()?;
                let sender_key = message
                    .sender_id
                    .as_ref()
                    .map(|id| {
                        key.opaque(
                            "sender",
                            &(&input.source.platform, &input.source.account_local_id, id),
                        )
                    })
                    .transpose()?;
                let metadata = message
                    .metadata
                    .as_ref()
                    .ok_or_else(|| invalid("missing metadata"))?;
                let edited = message
                    .edited_at
                    .as_ref()
                    .map(|v| scan.clean(v, "edited_at", Some(evidence), Some(&input.source)))
                    .transpose()?;
                let service_action = message
                    .service_action
                    .as_ref()
                    .map(|v| scan.clean(v, "service_action", Some(evidence), Some(&input.source)))
                    .transpose()?;
                let service_title = message
                    .service_title
                    .as_ref()
                    .map(|v| scan.clean(v, "service_title", Some(evidence), Some(&input.source)))
                    .transpose()?;
                let attachments = message
                    .attachments
                    .iter()
                    .enumerate()
                    .map(|(i, _)| key.opaque("attachment", &(&message.key, i)))
                    .collect::<io::Result<Vec<_>>>()?;
                let row = serde_json::json!({
                    "schema_version": 1, "source": source, "evidence": evidence,
                    "topic": topic, "topic_known": topic.is_some(),
                    "sender": sender, "sender_key": sender_key,
                    "timestamp": timestamp, "text": text,
                    "edited_at": edited, "service_action": service_action,
                    "service_title": service_title, "attachment_ids": attachments,
                    "reply": message.reply_to.as_ref().and_then(|id| refs.get(id)),
                    "identity_quality": metadata.identity_quality,
                    "deletion_state": metadata.deletion_state,
                    "is_service": message.is_service,
                    "attachment_count": message.attachments.len()
                });
                serde_json::to_writer(&mut output, &row).map_err(invalid)?;
                output.write_all(b"\n")?;
                count += 1;
            }
        }
        if count != manifest.messages || scan.summary.needs_review != 0 {
            return Err(invalid("structured selection or privacy changed"));
        }
        output.flush()?;
        output.get_ref().sync_all()?;
        drop(output);
        // The second scan must not introduce a mapping missing from the reviewed
        // project; otherwise the outbound text and its reverse index disagree.
        let privacy = scan.finish(self, &project)?;
        if privacy.reference != project.pseudonyms {
            return Err(invalid("structured privacy mapping changed; prepare again"));
        }
        let coverage = destination.join("coverage.json");
        files::write_json(
            &coverage,
            &serde_json::json!({
                "schema_version": 1, "sources": manifest.sources,
                "selection": key.opaque("selection", &(&private.inputs.iter().map(|i| (&i.source, &i.selection.filter)).collect::<Vec<_>>(), &project.privacy_options, &project.custom_terms))?,
                "messages": count, "missing_is_deletion": false,
                "acquisition": "supplied_export_or_partial_desktop_observations"
            }),
        )?;
        [path, coverage]
            .iter()
            .map(|path| {
                Ok(BundleFile {
                    name: path.file_name().unwrap().to_string_lossy().into_owned(),
                    bytes: fs::metadata(path)?.len(),
                    sha256: files::digest_file(path, cancelled)?,
                })
            })
            .collect()
    }
}
