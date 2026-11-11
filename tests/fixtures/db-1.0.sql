/* WARNING: Script requires that SQLITE_DBCONFIG_DEFENSIVE be disabled */
PRAGMA foreign_keys=OFF;
BEGIN TRANSACTION;
CREATE TABLE memories (
  id            integer primary key,
  project       text not null,
  title         text not null,
  body          text not null,
  created       text not null,
  updated       text not null,
  event_start   text,
  event_end     text,
  source        text not null check (source in ('explicit','suggested','migrated')),
  status        text not null default 'active'
                check (status in ('active','superseded','draft','forgotten')),
  pinned        integer not null default 0,
  last_accessed text,
  access_count  integer not null default 0,
  supersedes_id integer references memories(id),
  ext_id        text unique,
  topic         text,  -- topic key; see `ingest::save_with_topic`
  -- Who sent this memory, when it was not written here, and when it
  -- arrived. Null for everything local. See `db::add_provenance_columns`.
  received_from text,
  received_at   text
, agent text, created_ts text, verified integer not null default 0);
INSERT INTO memories VALUES(1,'api','the payment provider is Stripe','the payment provider is Stripe','2026-11-10','2026-11-10',NULL,NULL,'explicit','superseded',0,NULL,0,NULL,NULL,'payment-provider',NULL,NULL,NULL,'2026-11-10T19:32:32.043285060-05:00',0);
INSERT INTO memories VALUES(2,'api','the payment provider is Adyen since November','the payment provider is Adyen since November','2026-11-10','2026-11-10',NULL,NULL,'explicit','active',0,NULL,0,1,NULL,'payment-provider',NULL,NULL,NULL,'2026-11-10T19:32:32.056380110-05:00',0);
INSERT INTO memories VALUES(3,'api','deploys run the migration before the code','deploys run the migration before the code','2026-11-10','2026-11-10',NULL,NULL,'explicit','active',0,NULL,0,NULL,NULL,NULL,NULL,NULL,NULL,'2026-11-10T19:32:32.066686621-05:00',1);
INSERT INTO memories VALUES(4,'api','una nota con acentos y ñ, para fijar el unicode','una nota con acentos y ñ, para fijar el unicode','2026-11-10','2026-11-10',NULL,NULL,'explicit','active',0,NULL,0,NULL,NULL,NULL,NULL,NULL,NULL,'2026-11-10T19:32:32.093385698-05:00',0);
INSERT INTO memories VALUES(5,'api','something another machine told us','something another machine told us','2026-11-10','2026-11-10',NULL,NULL,'explicit','draft',0,NULL,0,NULL,NULL,NULL,'claude@laptop','2026-11-10T19:32:32.134323992-05:00',NULL,'2026-11-10T19:32:32.133993579-05:00',0);
CREATE TABLE memory_tags (
  memory_id integer not null references memories(id) on delete cascade,
  tag       text not null,
  primary key (memory_id, tag)
);
INSERT INTO memory_tags VALUES(1,'decision');
INSERT INTO memory_tags VALUES(2,'decision');
INSERT INTO memory_tags VALUES(3,'ops');
CREATE TABLE memory_refs (
  memory_id integer not null references memories(id) on delete cascade,
  kind      text not null check (kind in ('ticket','pr','sha')),
  value     text not null,
  primary key (memory_id, kind, value)
);
CREATE TABLE chunks (
  id        integer primary key,
  memory_id integer not null references memories(id) on delete cascade,
  ord       integer not null,
  body      text not null,
  -- What gets indexed, which is not always what gets shown. For Latin text
  -- the two are identical; for Chinese and Japanese this holds the bigram
  -- form (see `chunk::segment_cjk`), because `unicode61` would otherwise
  -- index a whole clause as one token nobody can type.
  --
  -- It is a column and not a transformation in the trigger because
  -- `chunks_fts` is an external-content table: `insert into
  -- chunks_fts(chunks_fts) values ('rebuild')` re-reads this table, so a
  -- trigger-side transformation would be reverted by the next rebuild
  -- without a word.
  fts_body  text not null default ''
);
INSERT INTO chunks VALUES(1,1,0,'the payment provider is Stripe','the payment provider is Stripe');
INSERT INTO chunks VALUES(2,2,0,'the payment provider is Adyen since November','the payment provider is Adyen since November');
INSERT INTO chunks VALUES(3,3,0,'deploys run the migration before the code','deploys run the migration before the code');
INSERT INTO chunks VALUES(4,4,0,'una nota con acentos y ñ, para fijar el unicode','una nota con acentos y ñ, para fijar el unicode');
INSERT INTO chunks VALUES(5,5,0,'something another machine told us','something another machine told us');
PRAGMA writable_schema=ON;
INSERT INTO sqlite_schema(type,name,tbl_name,rootpage,sql)VALUES('table','chunks_fts','chunks_fts',0,'CREATE VIRTUAL TABLE chunks_fts using fts5(
  fts_body,
  content=''chunks'',
  content_rowid=''id'',
  tokenize="unicode61 remove_diacritics 2"
)');
CREATE TABLE IF NOT EXISTS 'chunks_fts_data'(id INTEGER PRIMARY KEY, block BLOB);
INSERT INTO chunks_fts_data VALUES(1,x'0522');
INSERT INTO chunks_fts_data VALUES(10,x'000000000105050005010101020101030101040101050101');
INSERT INTO chunks_fts_data VALUES(137438953473,x'000000360330697301020501077061796d656e740102030207726f76696465720102040106737472697065010206010374686501020204070c0c0b');
INSERT INTO chunks_fts_data VALUES(274877906945,x'0000004c0630616479656e0202060102697302020501086e6f76656d62657202020801077061796d656e740202030207726f7669646572020204010573696e63650202070103746865020202040a070d0c0c0a');
INSERT INTO chunks_fts_data VALUES(412316860417,x'0000004307306265666f72650302060104636f646503020801076465706c6f797303020201096d6967726174696f6e030205010372756e030203010374686503040405040b090c0e08');
INSERT INTO chunks_fts_data VALUES(549755813889,x'0000005808306163656e746f730402050103636f6e0402040102656c04020a010566696a617204020901016e04020702036f74610402030104706172610402080103756e61040202030569636f646504020b010179040206040c08070a060809080a');
INSERT INTO chunks_fts_data VALUES(687194767361,x'0000003a0830616e6f7468657205020301076d616368696e650502040109736f6d657468696e670502020104746f6c6405020501027573050206040c0c0e09');
CREATE TABLE IF NOT EXISTS 'chunks_fts_idx'(segid, term, pgno, PRIMARY KEY(segid, term)) WITHOUT ROWID;
INSERT INTO chunks_fts_idx VALUES(1,x'',2);
INSERT INTO chunks_fts_idx VALUES(2,x'',2);
INSERT INTO chunks_fts_idx VALUES(3,x'',2);
INSERT INTO chunks_fts_idx VALUES(4,x'',2);
INSERT INTO chunks_fts_idx VALUES(5,x'',2);
CREATE TABLE IF NOT EXISTS 'chunks_fts_docsize'(id INTEGER PRIMARY KEY, sz BLOB);
INSERT INTO chunks_fts_docsize VALUES(1,x'05');
INSERT INTO chunks_fts_docsize VALUES(2,x'07');
INSERT INTO chunks_fts_docsize VALUES(3,x'07');
INSERT INTO chunks_fts_docsize VALUES(4,x'0a');
INSERT INTO chunks_fts_docsize VALUES(5,x'05');
CREATE TABLE IF NOT EXISTS 'chunks_fts_config'(k PRIMARY KEY, v) WITHOUT ROWID;
INSERT INTO chunks_fts_config VALUES('version',4);
CREATE TABLE chunk_vectors (
  chunk_id integer primary key references chunks(id) on delete cascade,
  vec      blob not null   -- f32 little endian x dims, ya normalizado
);
CREATE TABLE meta (
  key text primary key,
  value text not null
);
CREATE TABLE search_log (
  id           integer primary key,
  ts           text not null,
  query        text not null,
  project      text,
  returned_ids text not null,
  budget_used  integer not null,
  followed_up  integer not null default 0,
  -- Which memory was opened, when it is known. Null in rows written before
  -- the column existed, where `followed_up` says that one was and not which.
  followed_up_id integer
);
CREATE TRIGGER chunks_ai after insert on chunks begin
  insert into chunks_fts(rowid, fts_body) values (new.id, new.fts_body);
end;
CREATE TRIGGER chunks_ad after delete on chunks begin
  insert into chunks_fts(chunks_fts, rowid, fts_body) values ('delete', old.id, old.fts_body);
end;
CREATE TRIGGER chunks_au after update on chunks begin
  insert into chunks_fts(chunks_fts, rowid, fts_body) values ('delete', old.id, old.fts_body);
  insert into chunks_fts(rowid, fts_body) values (new.id, new.fts_body);
end;
CREATE INDEX idx_mem_project_status on memories(project, status);
CREATE INDEX idx_chunks_memory      on chunks(memory_id);
CREATE INDEX idx_tags_tag           on memory_tags(tag);
CREATE INDEX idx_refs_value         on memory_refs(value);
CREATE INDEX idx_mem_project_topic on memories(project, topic);
CREATE UNIQUE INDEX idx_mem_topic_activa
           on memories(project, topic)
           where status = 'active' and topic is not null;
CREATE INDEX idx_mem_project_created on memories(project, created_ts);
PRAGMA writable_schema=OFF;
COMMIT;
