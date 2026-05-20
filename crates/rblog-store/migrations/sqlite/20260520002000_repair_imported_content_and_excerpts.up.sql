-- Repair legacy Halo imports where the selected head/release snapshot only
-- stores an empty patch (`[]`) and the usable content remains in baseSnapshot.
UPDATE posts
SET
    markdown = COALESCE(
        NULLIF(markdown, '[]'),
        json_extract(CAST(bs.data AS TEXT), '$.spec.rawPatch'),
        markdown
    ),
    html = COALESCE(
        NULLIF(html, '[]'),
        json_extract(CAST(bs.data AS TEXT), '$.spec.contentPatch'),
        json_extract(CAST(bs.data AS TEXT), '$.spec.rawPatch'),
        html
    ),
    raw_type = COALESCE(json_extract(CAST(bs.data AS TEXT), '$.spec.rawType'), raw_type),
    excerpt = COALESCE(
        NULLIF(excerpt, ''),
        NULLIF(json_extract(CAST(p.data AS TEXT), '$.status.excerpt'), ''),
        excerpt
    )
FROM extensions p
LEFT JOIN extensions bs
  ON bs.name = '/registry/content.halo.run/snapshots/' || json_extract(CAST(p.data AS TEXT), '$.spec.baseSnapshot')
WHERE p.name = '/registry/content.halo.run/posts/' || posts.name
  AND json_extract(CAST(p.data AS TEXT), '$.kind') = 'Post'
  AND (
      markdown = '[]'
      OR html = '[]'
      OR excerpt IS NULL
      OR TRIM(excerpt) = ''
  );

UPDATE pages
SET
    markdown = COALESCE(
        NULLIF(markdown, '[]'),
        json_extract(CAST(bs.data AS TEXT), '$.spec.rawPatch'),
        markdown
    ),
    html = COALESCE(
        NULLIF(html, '[]'),
        json_extract(CAST(bs.data AS TEXT), '$.spec.contentPatch'),
        json_extract(CAST(bs.data AS TEXT), '$.spec.rawPatch'),
        html
    ),
    raw_type = COALESCE(json_extract(CAST(bs.data AS TEXT), '$.spec.rawType'), raw_type),
    excerpt = COALESCE(
        NULLIF(excerpt, ''),
        NULLIF(json_extract(CAST(p.data AS TEXT), '$.status.excerpt'), ''),
        excerpt
    )
FROM extensions p
LEFT JOIN extensions bs
  ON bs.name = '/registry/content.halo.run/snapshots/' || json_extract(CAST(p.data AS TEXT), '$.spec.baseSnapshot')
WHERE p.name = '/registry/content.halo.run/singlepages/' || pages.name
  AND json_extract(CAST(p.data AS TEXT), '$.kind') = 'SinglePage'
  AND (
      markdown = '[]'
      OR html = '[]'
      OR excerpt IS NULL
      OR TRIM(excerpt) = ''
  );
