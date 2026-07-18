use super::*;

#[test]
fn ignored_nonzero_resume_range_restarts_without_duplicating_partial_bytes() {
    let fixture = DownloadFixture::matching_manifest();
    fs::create_dir_all(fixture.store.staging_dir()).expect("create stable staging directory");
    fs::write(fixture.store.staging_dir().join("README.md.part"), b"tiny ").expect("seed part");
    fixture.fetcher.ignore_range_at("README.md", 5);

    fixture
        .run()
        .expect("ignored nonzero range safely restarts the file");

    assert_eq!(
        fs::read(fixture.store.qwen_install_dir().join("README.md"))
            .expect("published README exists"),
        b"tiny readme"
    );
    let offsets = fixture
        .fetcher
        .requests()
        .into_iter()
        .filter(|request| request.path == "README.md")
        .map(|request| request.range_start)
        .collect::<Vec<_>>();
    assert_eq!(offsets, vec![Some(5), Some(0)]);
    let progress = fixture
        .events
        .events()
        .into_iter()
        .filter(|event| event.phase == DownloadPhase::Downloading)
        .map(|event| event.downloaded_bytes)
        .collect::<Vec<_>>();
    assert!(progress.windows(2).any(|pair| pair == [5, 0]));
    assert_eq!(progress.last().copied(), Some(fixture.expected_bytes()));
}

#[test]
fn ignored_zero_offset_range_still_fails_closed() {
    let fixture = DownloadFixture::matching_manifest();
    fixture.fetcher.ignore_range_at("README.md", 0);

    assert!(matches!(
        fixture.run(),
        Err(DownloadError::InvalidContentRange { actual: None, .. })
    ));
    assert_eq!(qwen_state(&fixture.store), CatalogState::Absent);
}

#[test]
fn only_one_ignored_nonzero_range_restart_is_allowed_per_file() {
    let long_readme = vec![b'x'; MAX_RESPONSE_BYTES as usize + 9];
    let fixture = DownloadFixture::with_files(
        "repeated-ignored-range",
        BTreeMap::from([
            ("README.md".to_string(), long_readme),
            ("config.json".to_string(), b"{}".to_vec()),
            ("model.safetensors".to_string(), b"weight".to_vec()),
        ]),
    );
    fs::create_dir_all(fixture.store.staging_dir()).expect("create stable staging directory");
    fs::write(fixture.store.staging_dir().join("README.md.part"), b"stale")
        .expect("seed resumable part");
    fixture.fetcher.ignore_every_nonzero_range("README.md");
    fixture.fetcher.respect_range_end("README.md");
    fixture.fetcher.limit_requests(6);

    let result = fixture.run();
    assert!(
        matches!(
            result,
            Err(DownloadError::InvalidContentRange { actual: None, .. })
        ),
        "unexpected result: {result:?}"
    );
    let offsets = fixture
        .fetcher
        .requests()
        .into_iter()
        .filter(|request| request.path == "README.md")
        .map(|request| request.range_start)
        .collect::<Vec<_>>();
    assert_eq!(offsets, vec![Some(5), Some(0), Some(MAX_RESPONSE_BYTES)]);
}

#[test]
fn cancelled_resume_can_retry_through_an_ignored_nonzero_range() {
    let fixture = DownloadFixture::matching_manifest();
    fs::create_dir_all(fixture.store.staging_dir()).expect("create stable staging directory");
    fs::write(fixture.store.staging_dir().join("README.md.part"), b"tiny ").expect("seed part");
    let operation = fixture
        .registry
        .begin_download_for_store(&fixture.store, QWEN_CATALOG_ID)
        .expect("begin first download");
    fixture.fetcher.replace_plan(
        "README.md",
        FakePlan {
            bytes: b"tiny readme".to_vec(),
            redirects: Vec::new(),
            content_range: None,
            cancel_after_request: Some(operation.cancel.clone()),
        },
    );
    let manager = fixture.manager();
    let cancelled = manager.run(QWEN_CATALOG_ID, &operation);
    fixture.registry.finish(&operation);
    manager.emit_terminal(&operation, &cancelled);
    assert!(matches!(cancelled, Err(DownloadError::Cancelled)));
    assert_eq!(
        fixture.events.events().last().map(|event| event.phase),
        Some(DownloadPhase::Cancelled)
    );

    fixture.fetcher.replace_plan(
        "README.md",
        FakePlan {
            bytes: b"tiny readme".to_vec(),
            redirects: Vec::new(),
            content_range: None,
            cancel_after_request: None,
        },
    );
    fixture.fetcher.ignore_range_at("README.md", 5);
    fixture
        .run()
        .expect("retry restarts safely after the ignored resume range");
    assert_eq!(
        fs::read(fixture.store.qwen_install_dir().join("README.md"))
            .expect("published README exists"),
        b"tiny readme"
    );
}
