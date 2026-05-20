-- Remove Halo/plugin management records that rblog does not read.
--
-- The content records are kept for the next step while services move from
-- `extensions` to the clean relational tables. The deleted kinds are either
-- Halo console internals, notification queues, device sessions, plugin
-- metadata, role templates, or old metric counters that are not part of rblog's
-- product surface.

DELETE FROM extensions
WHERE json_extract(CAST(data AS TEXT), '$.kind') IN (
    'AuthProvider',
    'Counter',
    'Device',
    'ExtensionDefinition',
    'ExtensionPointDefinition',
    'Group',
    'Notification',
    'NotificationTemplate',
    'NotifierDescriptor',
    'Plugin',
    'Policy',
    'PolicyTemplate',
    'Reason',
    'ReasonType',
    'ReverseProxy',
    'Secret',
    'Subscription',
    'Theme'
);

DELETE FROM extensions
WHERE json_extract(CAST(data AS TEXT), '$.kind') = 'Role'
  AND json_extract(CAST(data AS TEXT), '$.metadata.name') LIKE 'role-template-%';
