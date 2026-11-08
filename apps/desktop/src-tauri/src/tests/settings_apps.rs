use super::*;

#[test]
fn the_redirect_reads_its_code_or_why_it_failed() {
    assert_eq!(
        redirect("/callback?code=a%2Bb&state=s1&iss=https%3A%2F%2Fid.example"),
        Redirect::Code {
            code: "a+b".into(),
            state: "s1".into(),
            issuer: Some("https://id.example".into()),
        }
    );
    assert_eq!(
        redirect("/callback?error=access_denied"),
        Redirect::Refused("access_denied".into())
    );
    assert_eq!(redirect("/favicon.ico"), Redirect::Other);
}
