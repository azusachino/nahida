# Reading fixtures

`two-pages.pdf` is a synthetic PDF 1.4 document created for Nahida's tests. It
contains a catalog, two page objects, a standard Helvetica font, two uncompressed
text streams and a complete cross-reference table. Page 1 says `First page:
anchored evidence.`; page 2 says `Second page: another passage.` No third-party
content, embedded scripts, external resources or credentials are present.

`article.html` is a synthetic two-section article. It exercises heading order,
inline text, HTML entities, Unicode, page chrome, hidden material and a script
that must never execute. The script's `.example` URL is test content only.
