use super::*;

#[tokio::test]
async fn every_page_is_read_until_one_is_empty() {
    let pages = [vec![json!(1), json!(2)], vec![json!(3)], vec![json!(4)]];
    let mut asked = Vec::new();
    let read = read_pages(|page| {
        asked.push(page);
        let index = usize::try_from(page - 1).expect("a small page number");
        let found = pages.get(index).cloned().unwrap_or_default();
        async move { Ok(found) }
    })
    .await
    .expect("the pages read");
    assert_eq!(read, [json!(1), json!(2), json!(3), json!(4)]);
    assert_eq!(asked, [1, 2, 3, 4]);
}

#[tokio::test]
async fn reading_stops_at_the_page_limit() {
    let mut asked = 0;
    let read = read_pages(|_| {
        asked += 1;
        let found = vec![json!(asked)];
        async move { Ok(found) }
    })
    .await
    .expect("the pages read");
    assert_eq!(
        read.len(),
        usize::try_from(GITHUB_MAX_PAGES).expect("a small limit")
    );
    assert_eq!(asked, GITHUB_MAX_PAGES);
}
