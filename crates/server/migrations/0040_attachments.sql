-- Files other than pictures that users send with a message, such as a PDF.
-- Like pictures they point at their message and stay as long as it.
ALTER TABLE files
    ADD COLUMN attached BOOLEAN NOT NULL DEFAULT false,
    ADD COLUMN size BIGINT NOT NULL DEFAULT 0;
