-- Existing attempts use the original generic JSON wire/parser. New adapters
-- freeze their versioned identity beside the unchanged exact outbound bytes.
ALTER TABLE scope_anti_bloat_reviews ADD COLUMN request_adapter_identity text
    NOT NULL DEFAULT 'generic-json-v1' CHECK (length(request_adapter_identity) > 0);

CREATE FUNCTION scope_anti_bloat_adapter_guard() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    IF OLD.request_bytes IS NOT NULL AND
       NEW.request_adapter_identity IS DISTINCT FROM OLD.request_adapter_identity THEN
        RAISE EXCEPTION 'anti-bloat adapter identity is immutable';
    END IF;
    RETURN NEW;
END;
$$;
CREATE TRIGGER scope_anti_bloat_adapter_guard BEFORE UPDATE ON scope_anti_bloat_reviews
    FOR EACH ROW EXECUTE FUNCTION scope_anti_bloat_adapter_guard();
REVOKE ALL PRIVILEGES ON FUNCTION scope_anti_bloat_adapter_guard() FROM PUBLIC;
