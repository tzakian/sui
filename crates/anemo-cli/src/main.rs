// Copyright (c) Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use narwhal_types::*;

#[tokio::main]
async fn main() {
    let config = anemo_cli::Config::new().add_service(
        "PrimaryToPrimary",
        anemo_cli::ServiceInfo::new()
            .add_method(
                "SendMessage",
                anemo_cli::ron_method!(PrimaryToPrimaryClient, send_message, PrimaryMessage),
            )
            .add_method(
                "RequestVote",
                anemo_cli::ron_method!(PrimaryToPrimaryClient, request_vote, RequestVoteRequest),
            )
            .add_method(
                "GetPayloadAvailability",
                anemo_cli::ron_method!(
                    PrimaryToPrimaryClient,
                    get_payload_availability,
                    PayloadAvailabilityRequest
                ),
            )
            .add_method(
                "GetCertificates",
                anemo_cli::ron_method!(
                    PrimaryToPrimaryClient,
                    get_certificates,
                    GetCertificatesRequest
                ),
            )
            .add_method(
                "FetchCertificates",
                anemo_cli::ron_method!(
                    PrimaryToPrimaryClient,
                    fetch_certificates,
                    FetchCertificatesRequest
                ),
            ),
    );
    anemo_cli::main(config).await;
}
