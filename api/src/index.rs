// Copyright (c) Aptos
// SPDX-License-Identifier: Apache-2.0

use crate::{
    accounts,
    context::Context,
    events,
    failpoint::fail_point,
    log,
    metrics::{metrics, status_metrics},
    transactions,
};
use aptos_api_types::{Error, Response};

use futures_util::StreamExt;
use std::convert::Infallible;
use std::time::Duration;
use tokio::time::interval;
use tokio_stream::wrappers::IntervalStream;
use warp::{
    body::BodyDeserializeError,
    cors::CorsForbidden,
    filters::BoxedFilter,
    http::{header, HeaderValue, StatusCode},
    reject::{LengthRequired, MethodNotAllowed, PayloadTooLarge, UnsupportedMediaType},
    reply,
    sse::Event,
    Filter, Rejection, Reply,
};

const OPEN_API_HTML: &str = include_str!("../doc/spec.html");
const OPEN_API_SPEC: &str = include_str!("../doc/openapi.yaml");

pub fn routes(context: Context) -> impl Filter<Extract = impl Reply, Error = Infallible> + Clone {
    index(context.clone())
        .or(openapi_spec())
        .or(sse(context.clone()))
        .or(accounts::get_account(context.clone()))
        .or(accounts::get_account_resources(context.clone()))
        .or(accounts::get_account_resource(context.clone()))
        .or(accounts::get_account_modules(context.clone()))
        .or(accounts::get_account_module(context.clone()))
        .or(accounts::get_account_state_blob(context.clone()))
        .or(transactions::get_transaction(context.clone()))
        .or(transactions::get_transactions(context.clone()))
        .or(transactions::get_account_transactions(context.clone()))
        .or(transactions::submit_bcs_transactions(context.clone()))
        .or(transactions::submit_json_transactions(context.clone()))
        .or(transactions::create_signing_message(context.clone()))
        .or(events::get_events_by_event_key(context.clone()))
        .or(events::get_events_by_event_handle(context.clone()))
        .or(context.health_check_route().with(metrics("health_check")))
        .with(
            warp::cors()
                .allow_any_origin()
                .allow_methods(vec!["POST", "GET"])
                .allow_headers(vec![header::CONTENT_TYPE]),
        )
        .recover(handle_rejection)
        .with(log::logger())
        .with(status_metrics())
}

// GET /openapi.yaml
// GET /spec.html
pub fn openapi_spec() -> BoxedFilter<(impl Reply,)> {
    let spec = warp::path!("openapi.yaml")
        .and(warp::get())
        .map(|| OPEN_API_SPEC)
        .with(metrics("openapi_yaml"))
        .boxed();
    let html = warp::path!("spec.html")
        .and(warp::get())
        .map(|| reply::html(open_api_html()))
        .with(metrics("spec_html"))
        .boxed();
    spec.or(html).boxed()
}

// GET /
pub fn index(context: Context) -> BoxedFilter<(impl Reply,)> {
    warp::path::end()
        .and(warp::get())
        .and(context.filter())
        .and_then(handle_index)
        .with(metrics("get_ledger_info"))
        .boxed()
}

pub async fn handle_index(context: Context) -> Result<impl Reply, Rejection> {
    fail_point("endpoint_index")?;
    let info = context.get_latest_ledger_info()?;
    Ok(Response::new(info.clone(), &info)?)
}

// GET /sse
pub fn sse(context: Context) -> BoxedFilter<(impl Reply,)> {
    warp::path!("sse")
        .and(warp::get())
        .and(context.filter())
        .and_then(handle_sse)
        .with(metrics("sse"))
        .boxed()
}

pub async fn handle_sse(context: Context) -> Result<impl Reply, Rejection> {
    fail_point("endpoint_sse")?;
    let interval = interval(Duration::from_secs(1));

    let info_stream = IntervalStream::new(interval).map(move |_| {
        let info = context.get_latest_ledger_info().unwrap();
        Event::default().json_data(info)
    });

    Ok(warp::sse::reply(info_stream))
}

async fn handle_rejection(err: Rejection) -> Result<impl Reply, Infallible> {
    let code;
    let body;

    if err.is_not_found() {
        code = StatusCode::NOT_FOUND;
        body = reply::json(&Error::new(code, "Not Found".to_owned()));
    } else if let Some(error) = err.find::<Error>() {
        code = error.status_code();
        body = reply::json(error);
    } else if let Some(cause) = err.find::<CorsForbidden>() {
        code = StatusCode::FORBIDDEN;
        body = reply::json(&Error::new(code, cause.to_string()));
    } else if let Some(cause) = err.find::<BodyDeserializeError>() {
        code = StatusCode::BAD_REQUEST;
        body = reply::json(&Error::new(code, cause.to_string()));
    } else if let Some(cause) = err.find::<LengthRequired>() {
        code = StatusCode::LENGTH_REQUIRED;
        body = reply::json(&Error::new(code, cause.to_string()));
    } else if let Some(cause) = err.find::<PayloadTooLarge>() {
        code = StatusCode::PAYLOAD_TOO_LARGE;
        body = reply::json(&Error::new(code, cause.to_string()));
    } else if let Some(cause) = err.find::<UnsupportedMediaType>() {
        code = StatusCode::UNSUPPORTED_MEDIA_TYPE;
        body = reply::json(&Error::new(code, cause.to_string()));
    } else if let Some(cause) = err.find::<MethodNotAllowed>() {
        code = StatusCode::METHOD_NOT_ALLOWED;
        body = reply::json(&Error::new(code, cause.to_string()));
    } else {
        code = StatusCode::INTERNAL_SERVER_ERROR;
        body = reply::json(&Error::new(code, format!("unexpected error: {:?}", err)));
    }
    let mut rep = reply::with_status(body, code).into_response();
    rep.headers_mut()
        .insert("access-control-allow-origin", HeaderValue::from_static("*"));
    Ok(rep)
}

fn open_api_html() -> String {
    OPEN_API_HTML.replace("hideTryIt=\"true\"", "")
}
