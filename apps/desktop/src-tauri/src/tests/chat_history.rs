use super::*;

#[test]
fn with_labels_prefers_the_live_description() {
    let turns = vec![Turn::Assistant {
        html: String::new(),
        tools: vec![
            tool_step("memory", "c1", r#"{"stored":true}"#, true),
            tool_step("memory", "c2", r#"{"stored":true}"#, true),
        ],
        replies: Vec::new(),
        model: String::new(),
    }];
    let labels = HashMap::from([("c1".to_owned(), "Remembering your flight".to_owned())]);
    let Turn::Assistant { tools, .. } = &with_labels(turns, labels)[0] else {
        panic!("an assistant turn stays one");
    };
    assert_eq!(tools[0].label.as_deref(), Some("Remembering your flight"));
    assert_eq!(tools[1].label.as_deref(), Some("Remembering"));
}

#[test]
fn sent_file_reads_send_file_results_only() {
    let output = r#"{"action":"send_file","file":{"id":"f1","name":"Pass.pdf","mime":"application/pdf","size":26000}}"#;
    let file = sent_file(SEND_FILE, output).expect("a send_file result names its file");
    assert_eq!(file.id, "f1");
    assert_eq!(file.size, 26000);
    assert!(sent_file("web_search", output).is_none());
    assert!(sent_file(SEND_FILE, "failed").is_none());
}

#[test]
fn sent_images_reads_urls_and_kept_files() {
    let output = r#"{"action":"send_image","images":[{"url":"https://pics.example/a.jpg","caption":"A"},{"file":{"id":"f2","name":"chart.png","mime":"image/png","size":900},"caption":""}],"failed":[]}"#;
    let images = sent_images(SEND_IMAGE, output);
    assert_eq!(images.len(), 2);
    assert!(
        matches!(&images[0].source, ImageSource::Url(url) if url == "https://pics.example/a.jpg")
    );
    assert_eq!(images[0].caption, "A");
    assert!(matches!(&images[1].source, ImageSource::File(file) if file.id == "f2"));
    assert!(sent_images(SEND_FILE, output).is_empty());
    assert!(sent_images(SEND_IMAGE, "failed").is_empty());
}

#[test]
fn made_media_shows_like_sent_media() {
    let image = r#"{"action":"send_image","images":[{"file":{"id":"f3","name":"cat.png","mime":"image/png","size":900},"caption":""}],"provider":"OpenAI","model":"gpt-image-2","path":"cat.png"}"#;
    let images = sent_images("image_gen", image);
    assert_eq!(images.len(), 1);
    assert!(matches!(&images[0].source, ImageSource::File(file) if file.id == "f3"));
    let model = r#"{"action":"send_file","file":{"id":"f4","name":"cat.glb","mime":"model/gltf-binary","size":4000},"provider":"Meshy","model":"latest","path":"cat.glb"}"#;
    for tool in ["video_gen", "3d_gen"] {
        let file = sent_file(tool, model).expect("a made file is named like a sent one");
        assert_eq!(file.id, "f4");
    }
}

#[test]
fn shown_products_reads_show_products_results_only() {
    let output = r#"{"action":"show_products","products":[{"title":"Kettle","url":"https://shop.example/k","store":"shop.example","price":19.5,"currency":"USD"}]}"#;
    let row = tool_step(SHOW_PRODUCTS, "c1", output, true)
        .products
        .expect("a show_products result has cards");
    assert!(row.contains("<b>$19.50</b>"), "{row}");
    assert!(tool_step(SEND_FILE, "c1", output, true).products.is_none());
    assert!(
        tool_step(SHOW_PRODUCTS, "c1", "failed", true)
            .products
            .is_none()
    );
}

#[test]
fn shown_chart_reads_chart_results_only() {
    let output = r#"{"action":"show_stock_chart","chart":{"title":"AAPL","kind":"area","x":"time","categories":[],"unit":"USD","series":[{"name":"AAPL","points":[{"x":1.0,"y":2.0}]}],"stock":null,"source":"Yahoo Finance"}}"#;
    let chart = shown_chart(SHOW_STOCK_CHART, output).expect("a chart result has a chart");
    assert_eq!(chart.series[0].points.len(), 1);
    assert!(shown_chart(SHOW_PRODUCTS, output).is_none());
    assert!(shown_chart(SHOW_CHART, "failed").is_none());
}
