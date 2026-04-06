// @generated automatically by Diesel CLI.

diesel::table! {
    batches (id) {
        id -> Uuid,
        status -> Text,
        job_count -> Int4,
        aggregated_proof_path -> Nullable<Text>,
        tx_hash -> Nullable<Text>,
        gas_used -> Nullable<Int8>,
        created_at -> Timestamptz,
        aggregated_at -> Nullable<Timestamptz>,
        settled_at -> Nullable<Timestamptz>,
    }
}

diesel::table! {
    jobs (id) {
        id -> Uuid,
        model_id -> Uuid,
        status -> Text,
        input_hash -> Text,
        proof_path -> Nullable<Text>,
        error -> Nullable<Text>,
        submitted_at -> Timestamptz,
        started_at -> Nullable<Timestamptz>,
        completed_at -> Nullable<Timestamptz>,
        settled_at -> Nullable<Timestamptz>,
        tx_hash -> Nullable<Text>,
        batch_id -> Nullable<Uuid>,
        attestation_hash -> Nullable<Text>,
        proof_bytes -> Nullable<Bytea>,
    }
}

diesel::table! {
    models (id) {
        id -> Uuid,
        name -> Text,
        version -> Text,
        ipfs_cid -> Text,
        input_shape -> Jsonb,
        on_chain_hash -> Text,
        registered_at -> Timestamptz,
    }
}

diesel::joinable!(jobs -> batches (batch_id));
diesel::joinable!(jobs -> models (model_id));

diesel::allow_tables_to_appear_in_same_query!(batches, jobs, models,);
