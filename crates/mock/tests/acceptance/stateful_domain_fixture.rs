//! Fresh-project fixture for the stateful authoring acceptance scenario.

use std::fs;

use chigiri_mock::hooks::HookSlot;
use chigiri_mock::luau::{render_hook_module, render_module};
use chigiri_mock::project::{Layout, read_project_schema};

use crate::support::Directory;

const PUBLIC: &str = r#"
type Image { id:ID! url:String! viewerLabel:String! @virtual }
type Wallet { id:ID! balance:Int! rewards:RewardConnection! @virtual }
type Reward { id:ID! walletId:ID! postId:ID! post:Post! @relation(fields:["postId"],references:["id"],name:"RewardPost") }
type RewardEdge { node:Reward! }
type RewardConnection { edges:[RewardEdge!]! }
type Post { id:ID! rewardId:ID! reward:Reward! @relation(fields:["rewardId"],references:["id"],name:"PostReward") }
type PostEdge { node:Post! }
type PostConnection { edges:[PostEdge!]! }
type User {
  id:ID! name:String! phone:String! walletId:ID!
  wallet:Wallet! @relation(fields:["walletId"],references:["id"])
  images:[Image!]!
  featuredImageId:ID!
  featuredImage:Image! @relation(fields:["featuredImageId"],references:["id"])
}
type Upload { id:ID! ownerId:ID! declaredSize:Int! completed:Boolean! myVote:Int @virtual }
type Transfer { id:ID! fromId:ID! toId:ID! amount:Int! idempotencyKey:String! }
type Query { me:User! upload(id:ID!):Upload history:[Transfer!]! feed:PostConnection! }
type Mutation {
  verify(phone:String!,purpose:String!,code:String!):String!
  createUpload(size:Int!):Upload!
  completeUpload(id:ID!):Upload!
  vote(uploadId:ID!,value:Int!):Upload!
  transfer(toId:ID!,amount:Int!,key:String!):Transfer!
  renameImage(id:ID!,url:String!):Image!
  failProvenance:Image!
}
"#;

const STORAGE: &str = r#"
extend type User { secret:String! }
extend type Image { ownerId:ID! }
scalar MockEpoch @representation(type: INTEGER)
type MockChallenge {
  id:ID! phone:String! purpose:String! code:String! expiresAt:MockEpoch!
  consumed:Boolean! userSnapshot:User!
}
type MockSession { id:ID! token:String! userId:ID! }
type MockVote { id:ID! uploadId:ID! userId:ID! value:Int! }
type MockTransferKey { id:ID! key:String! payload:String! transferId:ID! }
"#;

pub(super) fn build() -> Directory {
    let directory = Directory::new();
    fs::write(directory.0.join("schema.graphql"), PUBLIC).unwrap();
    let init = directory.command(&["init", "--schema", "schema.graphql", "--config-dir", "mock"]);
    assert!(
        init.status.success(),
        "{}",
        String::from_utf8_lossy(&init.stderr)
    );
    let root = directory.0.join("mock");
    fs::write(root.join("storage.graphql"), STORAGE).unwrap();
    let config = fs::read_to_string(root.join("mock.luau"))
        .unwrap()
        .replace(
            "field = { before = false, after = false }",
            "field = { before = false, after = true }",
        )
        .replace(
            "clock = { mode = \"system\" }",
            "clock = { mode = \"fixed\", now = 1789776000 }",
        )
        .replace("127.0.0.1:4000", "127.0.0.1:0");
    fs::write(root.join("mock.luau"), config).unwrap();
    let update = directory.command(&["update", "--config-dir", "mock"]);
    assert!(
        update.status.success(),
        "{}",
        String::from_utf8_lossy(&update.stderr)
    );
    write_seeds(&root);
    let schema = read_project_schema(&root).unwrap();
    let ir = schema.ir();
    let layout = Layout::new(ir).unwrap();
    for resolver in layout.resolvers {
        let body = body(&resolver.coordinate);
        let module = render_module(ir, resolver.script, &body).unwrap();
        fs::write(root.join(resolver.path.as_str()), module).unwrap();
    }
    let hook = render_hook_module(
        HookSlot::FieldAfter,
        r#"
local request = ctx.request :: ChigiriHookRequest
for _, header in request.headers do
    if ctx.field.coordinate == "Mutation.transfer" and header.name == "x-after-fail" then
        error("private after-hook failure")
    end
end
return nil
"#,
    )
    .unwrap();
    fs::write(root.join(HookSlot::FieldAfter.path()), hook).unwrap();
    directory
}

fn write_seeds(root: &std::path::Path) {
    fs::write(
        root.join("seeds/User.jsonl"),
        concat!(
            r#"{"id":"u1","name":"Ada","phone":"111","walletId":"w1","images":[{"$ref":{"model":"Image","key":{"id":"i1"}}}],"featuredImageId":"i1","secret":"alpha"}"#,
            "\n",
            r#"{"id":"u2","name":"Ben","phone":"222","walletId":"w2","images":[{"$ref":{"model":"Image","key":{"id":"i2"}}}],"featuredImageId":"i2","secret":"beta"}"#,
            "\n"
        ),
    )
    .unwrap();
    fs::write(
        root.join("seeds/Wallet.jsonl"),
        "{\"id\":\"w1\",\"balance\":100}\n{\"id\":\"w2\",\"balance\":20}\n",
    )
    .unwrap();
    fs::write(
        root.join("seeds/Image.jsonl"),
        "{\"id\":\"i1\",\"url\":\"one.png\",\"ownerId\":\"u1\"}\n{\"id\":\"i2\",\"url\":\"two.png\",\"ownerId\":\"u2\"}\n",
    )
    .unwrap();
    fs::write(
        root.join("seeds/Reward.jsonl"),
        "{\"id\":\"r1\",\"walletId\":\"w1\",\"postId\":\"p1\"}\n",
    )
    .unwrap();
    fs::write(
        root.join("seeds/Post.jsonl"),
        "{\"id\":\"p1\",\"rewardId\":\"r1\"}\n",
    )
    .unwrap();
    fs::write(root.join("seeds/Upload.jsonl"), "").unwrap();
    fs::write(root.join("seeds/Transfer.jsonl"), "").unwrap();
    fs::write(
        root.join("seeds/MockChallenge.jsonl"),
        concat!(
            r#"{"id":"c1","phone":"111","purpose":"LOGIN","code":"111111","expiresAt":1789776060,"consumed":false,"userSnapshot":{"$ref":{"model":"User","key":{"id":"u1"}}}}"#,
            "\n",
            r#"{"id":"c2","phone":"222","purpose":"LOGIN","code":"222222","expiresAt":1789776060,"consumed":false,"userSnapshot":{"$ref":{"model":"User","key":{"id":"u2"}}}}"#,
            "\n",
            r#"{"id":"c3","phone":"111","purpose":"RECOVERY","code":"333333","expiresAt":1789775999,"consumed":false,"userSnapshot":{"$ref":{"model":"User","key":{"id":"u1"}}}}"#,
            "\n",
            r#"{"id":"c4","phone":"111","purpose":"EXACT","code":"444444","expiresAt":1789776000,"consumed":false,"userSnapshot":{"$ref":{"model":"User","key":{"id":"u1"}}}}"#,
            "\n"
        ),
    )
    .unwrap();
    for model in ["MockSession", "MockVote", "MockTransferKey"] {
        fs::write(root.join(format!("seeds/{model}.jsonl")), "").unwrap();
    }
}

fn body(coordinate: &str) -> String {
    let authenticated = |tail: &str| format!("{TOKEN}\n{tail}");
    match coordinate {
        "Query.me" => authenticated(ME),
        "Query.upload" => "return ctx.models.Upload:find({where={field='id',op='eq',value=args.id}})[1] or ctx.null".into(),
        "Query.history" => authenticated(HISTORY),
        "Query.feed" => FEED.into(),
        "Upload.myVote" => authenticated(MY_VOTE),
        "Wallet.rewards" => WALLET_REWARDS.into(),
        "Image.viewerLabel" => IMAGE_LABEL.into(),
        "Mutation.verify" => VERIFY.into(),
        "Mutation.createUpload" => authenticated(CREATE_UPLOAD),
        "Mutation.completeUpload" => authenticated(COMPLETE_UPLOAD),
        "Mutation.vote" => authenticated(VOTE),
        "Mutation.transfer" => authenticated(TRANSFER),
        "Mutation.renameImage" => authenticated(RENAME_IMAGE),
        "Mutation.failProvenance" => authenticated(FAIL_PROVENANCE),
        _ => panic!("missing stateful resolver: {coordinate}"),
    }
}

const TOKEN: &str = r#"
local request = ctx.request
if request == ctx.null then ctx.fail({code="UNAUTHENTICATED"}) end
local http = request :: ChigiriHookRequest
local token: string? = nil
for _, header in http.headers do
    if header.name == "x-session" then token = header.value end
end
local required = token or ctx.fail({code="UNAUTHENTICATED"})
local session = ctx.models.MockSession:find({where={field="token",op="eq",value=required}})[1] or ctx.fail({code="UNAUTHENTICATED"})
"#;

const ME: &str = "return ctx.models.User:find({where={field='id',op='eq',value=session.userId}})[1] or ctx.fail({code='UNAUTHENTICATED'})";

const HISTORY: &str =
    "return ctx.models.Transfer:find({where={field='fromId',op='eq',value=session.userId}})";

const FEED: &str = r#"
local post = ctx.models.Post:find({where={field="id",op="eq",value="p1"}})[1] or ctx.fail({code="NOT_FOUND"})
return {edges={{node=post}}}
"#;

const WALLET_REWARDS: &str = r#"
local reward = ctx.models.Reward:find({where={field="walletId",op="eq",value=parent.id}})[1]
if reward == nil then return {edges={}} end
return {edges={{node=reward}}}
"#;

const IMAGE_LABEL: &str = r#"
local request = ctx.request
if request == ctx.null then return "anonymous:" .. parent.ownerId end
local http = request :: ChigiriHookRequest
for _, header in http.headers do
    if header.name == "x-viewer" then return header.value .. ":" .. parent.ownerId end
end
return "anonymous:" .. parent.ownerId
"#;

const MY_VOTE: &str = r#"
local rows = ctx.models.MockVote:find({where={field="uploadId",op="eq",value=parent.id}})
for _, vote in rows do if vote.userId == session.userId then return vote.value end end
return ctx.null
"#;

const VERIFY: &str = r#"
local rows = ctx.models.MockChallenge:find({where={field="phone",op="eq",value=args.phone}})
for _, challenge in rows do
    if challenge.purpose == args.purpose and challenge.code == args.code then
        if challenge.consumed then ctx.fail({code="CODE_CONSUMED"}) end
        if assert(os.time()) >= challenge.expiresAt then ctx.fail({code="CODE_EXPIRED"}) end
        ctx.models.MockChallenge:update({where={field="id",op="eq",value=challenge.id}},{consumed=true})
        local userId = challenge.userSnapshot.id
        local sequence = ctx.models.MockSession:count({}) + 1
        local token = "opaque-token-" .. tostring(sequence)
        ctx.models.MockSession:create({id="opaque-session-"..tostring(sequence),token=token,userId=userId})
        return token
    end
end
return ctx.fail({code="INVALID_CODE"})
"#;

const CREATE_UPLOAD: &str = r#"
local id = "upload-" .. (ctx.models.Upload:count({}) + 1)
return ctx.models.Upload:create({id=id,ownerId=session.userId,declaredSize=args.size,completed=false})
"#;

const COMPLETE_UPLOAD: &str = r#"
local upload = ctx.models.Upload:find({where={field="id",op="eq",value=args.id}})[1]
if upload == nil then ctx.fail({code="NOT_FOUND"}) end
if upload.ownerId ~= session.userId then ctx.fail({code="NOT_OWNER"}) end
return ctx.models.Upload:update({where={field="id",op="eq",value=args.id}},{completed=true})[1]
"#;

const VOTE: &str = r#"
local existing = nil
local votes = ctx.models.MockVote:find({where={field="uploadId",op="eq",value=args.uploadId}})
for _, vote in votes do
    if vote.userId == session.userId then existing = vote end
end
if existing == nil then
    local id = "opaque-vote-" .. tostring(ctx.models.MockVote:count({}) + 1)
    ctx.models.MockVote:create({id=id,uploadId=args.uploadId,userId=session.userId,value=args.value})
else
    ctx.models.MockVote:update({where={field="id",op="eq",value=existing.id}},{value=args.value})
end
return ctx.models.Upload:find({where={field="id",op="eq",value=args.uploadId}})[1] or ctx.fail({code="NOT_FOUND"})
"#;

const TRANSFER: &str = r#"
local payload = session.userId .. ":" .. args.toId .. ":" .. tostring(args.amount)
local prior = ctx.models.MockTransferKey:find({where={field="key",op="eq",value=args.key}})[1]
if prior ~= nil then
    if prior.payload ~= payload then ctx.fail({code="IDEMPOTENCY_CONFLICT"}) end
    return ctx.models.Transfer:find({where={field="id",op="eq",value=prior.transferId}})[1] or ctx.fail({code="NOT_FOUND"})
end
local sender = ctx.models.User:find({where={field="id",op="eq",value=session.userId}})[1]
local receiver = ctx.models.User:find({where={field="id",op="eq",value=args.toId}})[1]
if sender == nil or receiver == nil then ctx.fail({code="NOT_FOUND"}) end
local senderWallet = ctx.models.Wallet:find({where={field="id",op="eq",value=sender.walletId}})[1]
local receiverWallet = ctx.models.Wallet:find({where={field="id",op="eq",value=receiver.walletId}})[1]
if senderWallet == nil or receiverWallet == nil then ctx.fail({code="NOT_FOUND"}) end
if args.amount <= 0 or senderWallet.balance < args.amount then ctx.fail({code="INSUFFICIENT_BALANCE"}) end
ctx.models.Wallet:update({where={field="id",op="eq",value=senderWallet.id}},{balance=senderWallet.balance-args.amount})
ctx.models.Wallet:update({where={field="id",op="eq",value=receiverWallet.id}},{balance=receiverWallet.balance+args.amount})
local id = "transfer-" .. (ctx.models.Transfer:count({}) + 1)
local transfer = ctx.models.Transfer:create({id=id,fromId=sender.id,toId=receiver.id,amount=args.amount,idempotencyKey=args.key})
local ledgerId = "opaque-ledger-" .. tostring(ctx.models.MockTransferKey:count({}) + 1)
ctx.models.MockTransferKey:create({id=ledgerId,key=args.key,payload=payload,transferId=id})
return transfer
"#;

const RENAME_IMAGE: &str = r#"
local image = ctx.models.Image:find({where={field="id",op="eq",value=args.id}})[1]
if image == nil then ctx.fail({code="NOT_FOUND"}) end
if image.ownerId ~= session.userId then ctx.fail({code="NOT_OWNER"}) end
return ctx.models.Image:update({where={field="id",op="eq",value=args.id}},{url=args.url,ownerId="rotated-"..session.userId})[1]
"#;

const FAIL_PROVENANCE: &str =
    "return {id='forged-image',url='private-projection',ownerId='PRIVATE_VALUE'} :: any";
