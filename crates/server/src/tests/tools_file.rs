use super::*;
use flashmind::tools::mcp::McpBlob;

#[test]
fn a_pdf_resource_is_read_from_its_file_block() {
    let pdf = b"%PDF-1.7 pass";
    let result = McpToolCallResult {
        content: vec![McpContent::Blob(McpBlob {
            uri: Some("fastmail:blob".into()),
            mime_type: Some("application/pdf".into()),
            data: STANDARD.encode(pdf),
        })],
        is_error: false,
    };
    let found = attachment_in(result).expect("a file");
    assert_eq!(found.data, pdf);
    assert_eq!(found.mime.as_deref(), Some("application/pdf"));
}

#[test]
fn names_lose_folders_and_leading_dots() {
    assert_eq!(clean_name("../../etc/passwd"), "passwd");
    assert_eq!(clean_name(".hidden"), "hidden");
    assert_eq!(clean_name("  "), "file");
    assert_eq!(clean_name("Boarding Pass.pdf"), "Boarding Pass.pdf");
}
