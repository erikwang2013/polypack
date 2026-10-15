//! iOS 打包器（T-M2-08）：Xcode 工程骨架生成/复用 + `xcodebuild archive` + `exportArchive`
//! 直出未签名 IPA。
//!
//! - **工程来源**：`inputs.ios.host_project` 下已有 `<产品名>.xcodeproj`（RN 宿主模板，
//!   T-M2-04）即复用；否则生成最小工程骨架（pbxproj + Info.plist + App.swift +
//!   ExportOptions.plist），链接 [swift](T-M2-03) 的 XCFramework 与 Swift 绑定。
//! - **未签名产物**：archive 阶段 `CODE_SIGNING_ALLOWED=NO`（签名位由 T-M2-10 poly-sign
//!   iOS 链路接入）；导出方式经 `POLY_IOS_EXPORT_METHOD` 可配（缺省 `development`）。
//! - **宿主门控**：仅 macOS 宿主可执行（`xcodebuild` / `plutil` / `xcrun`）——非 macOS
//!   `package()` 在动手前返回 [`BundleError::ToolMissing`]；`plan()` 为纯组装，任意宿主可跑。
//! - 证书 / 描述文件只以凭据引用出现（T-M2-11 模型）：打包阶段不接触密钥材料。
//!
//! shortcut: 生成的工程骨架为静态文本（`objectVersion = 56`），未在 macOS 实证——
//! macos-14 CI nightly（T-M2-14）真跑 xcodebuild 校验；RN 宿主模板落地后走「复用」路径。

pub mod swift;

use std::path::{Path, PathBuf};

use poly_targets::TargetSpec;

use crate::error::BundleError;
use crate::exec;
use crate::fsutil;
use crate::inputs::{BundleInputs, IosInputs, ProductInfo};
use crate::packager::{Artifact, BundlePlan, Command, Packager, ScriptFile};

/// iOS IPA 打包器。
pub struct Ios;

/// 目标 / 打包器标识。
pub(crate) const IOS: &str = "ios";
/// 产物格式。
const IPA: &str = "ipa";

/// 导出方式环境变量（对应 ExportOptions.plist 的 `method` 键）。
pub const EXPORT_METHOD_ENV: &str = "POLY_IOS_EXPORT_METHOD";

/// 骨架工程的 app target 对象 id（pbxproj 与共享 scheme 的 `BlueprintIdentifier` 必须一致）。
const TARGET_ID: &str = "C0DE0000000000000000A002";

/// Xcode 认可的导出方式（Xcode 15.4 起 `app-store-connect` / `release-testing` / `debugging`
/// 为新名；`app-store` / `ad-hoc` / `development` 保留兼容）。
const EXPORT_METHODS: &[&str] = &[
    "app-store",
    "app-store-connect",
    "release-testing",
    "ad-hoc",
    "development",
    "debugging",
    "enterprise",
    "validation",
];

impl Packager for Ios {
    fn id(&self) -> &'static str {
        IOS
    }

    fn format(&self) -> &'static str {
        IPA
    }

    fn plan(&self, inputs: &BundleInputs, spec: &TargetSpec) -> Result<BundlePlan, BundleError> {
        check_format(spec, IPA)?;
        let ios = ios_inputs(inputs, spec)?;
        // XCFramework（T-M2-03）是必需输入：iOS 包缺桥产物 = 装起来必崩，响亮拒绝
        // （同 macOS .app 缺 icns 的先例，不静默省略）。
        if !swift::xcframework_path(inputs).is_dir() {
            return Err(BundleError::MissingInput {
                packager: IOS,
                field: "ios.xcframework",
            });
        }
        let method = export_method()?;
        // ExportOptions.plist 是 exportArchive 的必需输入：骨架与复用路径都要落盘。
        let mut plan = BundlePlan::new()
            .with_script(export_options_path(inputs), export_options_plist(&method));

        let project = match resolve_project(inputs, ios) {
            ProjectSource::Reuse(project) => {
                // 复用不落盘脚本：以命令流留痕（T-M2-04 RN 宿主模板；骨架生成跳过）。
                plan = plan.with_command(Command::new(
                    "echo",
                    ["复用宿主工程".into(), path_arg(&project)],
                ));
                project
            }
            ProjectSource::Skeleton(project) => {
                for script in skeleton_scripts(inputs, ios, &project)? {
                    plan = plan.with_script(script.path, script.contents);
                }
                project
            }
        };
        for command in build_commands(&project, inputs) {
            plan = plan.with_command(command);
        }
        Ok(plan)
    }

    fn package(&self, inputs: &BundleInputs, spec: &TargetSpec) -> Result<Artifact, BundleError> {
        // 宿主门控最前置：非 macOS 宿主无从谈起（先于一切输入校验，也先于落盘）。
        require_macos("xcodebuild")?;
        let plan = self.plan(inputs, spec)?;
        let method = export_method()?;
        // 输入契约校验（快速失败，不浪费 xcodebuild）：XCFramework 结构不合法即拒。
        swift::validate_xcframework_layout(&inputs.out_dir)?;

        fsutil::write_scripts(&plan)?;
        exec::run_all(&plan.commands)?;

        let export_dir = export_dir(inputs);
        let ipa = find_ipa(&export_dir).ok_or(BundleError::MissingInput {
            packager: IOS,
            field: "export/*.ipa",
        })?;
        fsutil::file_artifact(
            IOS,
            IPA,
            &ipa,
            vec![
                "未签名 IPA（archive 阶段 CODE_SIGNING_ALLOWED=NO；签名位由 T-M2-10 poly-sign \
                 iOS 链路接入）"
                    .to_string(),
                "证书 / 描述文件只以凭据引用出现（T-M2-11 模型）：打包阶段不接触密钥材料"
                    .to_string(),
                format!(
                    "导出方式 {method}（{} 可覆写）；archive 内含 Info.plist 与架构（plutil / \
                     xcrun lipo）校验",
                    EXPORT_METHOD_ENV
                ),
                "真机执行阻塞（宿主）：xcodebuild archive / exportArchive 仅 macOS 宿主可跑；\
                 macos-14 CI nightly 真跑（T-M2-14）"
                    .to_string(),
            ],
        )
    }
}

impl Ios {
    /// 由本打包器生成/复用的工程路径（供测试与后续签名阶段定位 archive）。
    pub fn project_path(
        &self,
        inputs: &BundleInputs,
        spec: &TargetSpec,
    ) -> Result<PathBuf, BundleError> {
        check_format(spec, IPA)?;
        let ios = ios_inputs(inputs, spec)?;
        Ok(match resolve_project(inputs, ios) {
            ProjectSource::Reuse(project) | ProjectSource::Skeleton(project) => project,
        })
    }
}

// ---------------------------------------------------------------- 工程来源

/// 工程来源：复用宿主工程 or 生成骨架。
enum ProjectSource {
    /// 宿主工程的 `<产品名>.xcodeproj`（RN 宿主模板，T-M2-04）。
    Reuse(PathBuf),
    /// 生成的工程骨架 `<out>/ios-skeleton/<产品名>.xcodeproj`。
    Skeleton(PathBuf),
}

/// 复用判定：`host_project`（或其 `ios/` 子目录）下已有 `<产品名>.xcodeproj` 即复用。
fn resolve_project(inputs: &BundleInputs, ios: &IosInputs) -> ProjectSource {
    let name = &inputs.product.name;
    if let Some(host) = &ios.host_project {
        let direct = host.join(format!("{name}.xcodeproj"));
        let nested = host.join("ios").join(format!("{name}.xcodeproj"));
        for candidate in [direct, nested] {
            if candidate.is_dir() {
                return ProjectSource::Reuse(candidate);
            }
        }
    }
    ProjectSource::Skeleton(skeleton_project_dir(inputs))
}

/// 骨架工程目录（`<out>/ios-skeleton/<产品名>.xcodeproj`）。
fn skeleton_project_dir(inputs: &BundleInputs) -> PathBuf {
    skeleton_root(inputs).join(format!("{}.xcodeproj", inputs.product.name))
}

/// 骨架根目录。
fn skeleton_root(inputs: &BundleInputs) -> PathBuf {
    inputs.out_dir.join("ios-skeleton")
}

// ---------------------------------------------------------------- 骨架生成

/// 骨架脚本集：pbxproj + 共享 scheme + Info.plist + App.swift。
fn skeleton_scripts(
    inputs: &BundleInputs,
    ios: &IosInputs,
    project: &Path,
) -> Result<Vec<ScriptFile>, BundleError> {
    let lib = ios
        .lib_name
        .as_deref()
        .filter(|name| !name.is_empty())
        .ok_or(BundleError::MissingInput {
            packager: IOS,
            field: "ios.lib_name",
        })?;
    let source_dir = skeleton_root(inputs).join(&inputs.product.name);
    Ok(vec![
        ScriptFile {
            path: project.join("project.pbxproj"),
            contents: skeleton_pbxproj(&inputs.product, lib, &swift::xcframework_path(inputs)),
        },
        ScriptFile {
            path: project
                .join("xcshareddata/xcschemes")
                .join(format!("{}.xcscheme", inputs.product.name)),
            contents: skeleton_scheme(&inputs.product),
        },
        ScriptFile {
            path: source_dir.join("Info.plist"),
            contents: skeleton_info_plist(&inputs.product),
        },
        ScriptFile {
            path: source_dir.join("App.swift"),
            contents: skeleton_app_swift(&inputs.product),
        },
    ])
}

/// 最小 `project.pbxproj`（单 app target；链接 XCFramework + Swift 绑定进 Sources）。
///
/// 工程位于 `<out>/ios-skeleton/`，故引用相对路径：`../<产品名>.xcframework` 与
/// `../bridge/swift/<lib>.swift`（均在 `<out>` 下）。
pub fn skeleton_pbxproj(product: &ProductInfo, lib_name: &str, xcframework: &Path) -> String {
    let name = &product.name;
    let xcframework_ref = format!(
        "../{}",
        xcframework
            .file_name()
            .map(|file| file.to_string_lossy().into_owned())
            .unwrap_or_else(|| format!("{name}.xcframework"))
    );
    format!(
        r#"// !$*UTF8*$!
{{
	archiveVersion = 1;
	classes = {{
	}};
	objectVersion = 56;
	objects = {{

/* Begin PBXBuildFile section */
		C0DE0000000000000000B001 /* App.swift in Sources */ = {{isa = PBXBuildFile; fileRef = C0DE0000000000000000F001 /* App.swift */; }};
		C0DE0000000000000000B002 /* {lib_name}.swift in Sources */ = {{isa = PBXBuildFile; fileRef = C0DE0000000000000000F004 /* {lib_name}.swift */; }};
		C0DE0000000000000000B003 /* {name}.xcframework in Frameworks */ = {{isa = PBXBuildFile; fileRef = C0DE0000000000000000F005 /* {name}.xcframework */; }};
/* End PBXBuildFile section */

/* Begin PBXFileReference section */
		C0DE0000000000000000F001 /* App.swift */ = {{isa = PBXFileReference; lastKnownFileType = sourcecode.swift; path = App.swift; sourceTree = "<group>"; }};
		C0DE0000000000000000F002 /* Info.plist */ = {{isa = PBXFileReference; lastKnownFileType = text.plist.xml; path = Info.plist; sourceTree = "<group>"; }};
		C0DE0000000000000000F003 /* {name}.app */ = {{isa = PBXFileReference; explicitFileType = wrapper.application; includeInIndex = 0; path = "{name}.app"; sourceTree = BUILT_PRODUCTS_DIR; }};
		C0DE0000000000000000F004 /* {lib_name}.swift */ = {{isa = PBXFileReference; lastKnownFileType = sourcecode.swift; name = "{lib_name}.swift"; path = "../bridge/swift/{lib_name}.swift"; sourceTree = "<group>"; }};
		C0DE0000000000000000F005 /* {name}.xcframework */ = {{isa = PBXFileReference; lastKnownFileType = wrapper.xcframework; name = "{name}.xcframework"; path = "{xcframework_ref}"; sourceTree = "<group>"; }};
/* End PBXFileReference section */

/* Begin PBXFrameworksBuildPhase section */
		C0DE0000000000000000D002 /* Frameworks */ = {{
			isa = PBXFrameworksBuildPhase;
			buildActionMask = 2147483647;
			files = (
				C0DE0000000000000000B003 /* {name}.xcframework in Frameworks */,
			);
			runOnlyForDeploymentPostprocessing = 0;
		}};
/* End PBXFrameworksBuildPhase section */

/* Begin PBXGroup section */
		C0DE0000000000000000A003 = {{
			isa = PBXGroup;
			children = (
				C0DE0000000000000000A004 /* {name} */,
				C0DE0000000000000000F004 /* {lib_name}.swift */,
				C0DE0000000000000000F005 /* {name}.xcframework */,
				C0DE0000000000000000A005 /* Products */,
			);
			// `../…` 相对 SRCROOT（.xcodeproj 所在目录 = <out>/ios-skeleton）解析：
			// XCFramework 在 <out>/、Swift 绑定在 <out>/bridge/swift —— 二者挂在主组。
			sourceTree = "<group>";
		}};
		C0DE0000000000000000A004 /* {name} */ = {{
			isa = PBXGroup;
			children = (
				C0DE0000000000000000F001 /* App.swift */,
				C0DE0000000000000000F002 /* Info.plist */,
			);
			path = "{name}";
			sourceTree = "<group>";
		}};
		C0DE0000000000000000A005 /* Products */ = {{
			isa = PBXGroup;
			children = (
				C0DE0000000000000000F003 /* {name}.app */,
			);
			name = Products;
			sourceTree = "<group>";
		}};
/* End PBXGroup section */

/* Begin PBXNativeTarget section */
		{target_id} /* {name} */ = {{
			isa = PBXNativeTarget;
			buildConfigurationList = C0DE0000000000000000E002 /* Build configuration list for PBXNativeTarget "{name}" */;
			buildPhases = (
				C0DE0000000000000000D001 /* Sources */,
				C0DE0000000000000000D002 /* Frameworks */,
				C0DE0000000000000000D003 /* Resources */,
			);
			buildRules = (
			);
			dependencies = (
			);
			name = "{name}";
			productName = "{name}";
			productReference = C0DE0000000000000000F003 /* {name}.app */;
			productType = "com.apple.product-type.application";
		}};
/* End PBXNativeTarget section */

/* Begin PBXProject section */
		C0DE0000000000000000A001 /* Project object */ = {{
			isa = PBXProject;
			attributes = {{
				BuildIndependentTargetsInParallel = 1;
				LastUpgradeCheck = 1500;
				TargetAttributes = {{
					{target_id} = {{
						CreatedOnToolsVersion = 15.0;
					}};
				}};
			}};
			buildConfigurationList = C0DE0000000000000000E001 /* Build configuration list for PBXProject "{name}" */;
			compatibilityVersion = "Xcode 14.0";
			developmentRegion = en;
			hasScannedForEncodings = 0;
			knownRegions = (
				en,
				Base,
			);
			mainGroup = C0DE0000000000000000A003;
			productRefGroup = C0DE0000000000000000A005 /* Products */;
			projectDirPath = "";
			projectRoot = "";
			targets = (
				{target_id} /* {name} */,
			);
		}};
/* End PBXProject section */

/* Begin PBXResourcesBuildPhase section */
		C0DE0000000000000000D003 /* Resources */ = {{
			isa = PBXResourcesBuildPhase;
			buildActionMask = 2147483647;
			files = (
			);
			runOnlyForDeploymentPostprocessing = 0;
		}};
/* End PBXResourcesBuildPhase section */

/* Begin PBXSourcesBuildPhase section */
		C0DE0000000000000000D001 /* Sources */ = {{
			isa = PBXSourcesBuildPhase;
			buildActionMask = 2147483647;
			files = (
				C0DE0000000000000000B001 /* App.swift in Sources */,
				C0DE0000000000000000B002 /* {lib_name}.swift in Sources */,
			);
			runOnlyForDeploymentPostprocessing = 0;
		}};
/* End PBXSourcesBuildPhase section */

/* Begin XCBuildConfiguration section */
		C0DE0000000000000000C001 /* Debug */ = {{
			isa = XCBuildConfiguration;
			buildSettings = {{
				ALWAYS_SEARCH_USER_PATHS = NO;
				CLANG_ENABLE_MODULES = YES;
				CLANG_ENABLE_OBJC_ARC = YES;
				ENABLE_STRICT_OBJC_MSGSEND = YES;
				IPHONEOS_DEPLOYMENT_TARGET = 16.0;
				ONLY_ACTIVE_ARCH = YES;
				SDKROOT = iphoneos;
				SWIFT_ACTIVE_COMPILATION_CONDITIONS = DEBUG;
				SWIFT_OPTIMIZATION_LEVEL = "-Onone";
				SWIFT_VERSION = 5.0;
			}};
			name = Debug;
		}};
		C0DE0000000000000000C002 /* Release */ = {{
			isa = XCBuildConfiguration;
			buildSettings = {{
				ALWAYS_SEARCH_USER_PATHS = NO;
				CLANG_ENABLE_MODULES = YES;
				CLANG_ENABLE_OBJC_ARC = YES;
				ENABLE_STRICT_OBJC_MSGSEND = YES;
				IPHONEOS_DEPLOYMENT_TARGET = 16.0;
				SDKROOT = iphoneos;
				SWIFT_COMPILATION_MODE = wholemodule;
				SWIFT_OPTIMIZATION_LEVEL = "-O";
				SWIFT_VERSION = 5.0;
			}};
			name = Release;
		}};
		C0DE0000000000000000C003 /* Debug */ = {{
			isa = XCBuildConfiguration;
			buildSettings = {{
				CODE_SIGNING_ALLOWED = NO;
				CODE_SIGNING_REQUIRED = NO;
				CODE_SIGN_IDENTITY = "";
				CURRENT_PROJECT_VERSION = 1;
				GENERATE_INFOPLIST_FILE = NO;
				INFOPLIST_FILE = "{name}/Info.plist";
				LD_RUNPATH_SEARCH_PATHS = (
					"$(inherited)",
					"@executable_path/Frameworks",
				);
				MARKETING_VERSION = {version};
				PRODUCT_BUNDLE_IDENTIFIER = {identifier};
				PRODUCT_NAME = "{name}";
				SWIFT_EMIT_LOC_STRINGS = NO;
				SWIFT_VERSION = 5.0;
				TARGETED_DEVICE_FAMILY = "1,2";
			}};
			name = Debug;
		}};
		C0DE0000000000000000C004 /* Release */ = {{
			isa = XCBuildConfiguration;
			buildSettings = {{
				CODE_SIGNING_ALLOWED = NO;
				CODE_SIGNING_REQUIRED = NO;
				CODE_SIGN_IDENTITY = "";
				CURRENT_PROJECT_VERSION = 1;
				GENERATE_INFOPLIST_FILE = NO;
				INFOPLIST_FILE = "{name}/Info.plist";
				LD_RUNPATH_SEARCH_PATHS = (
					"$(inherited)",
					"@executable_path/Frameworks",
				);
				MARKETING_VERSION = {version};
				PRODUCT_BUNDLE_IDENTIFIER = {identifier};
				PRODUCT_NAME = "{name}";
				SWIFT_EMIT_LOC_STRINGS = NO;
				SWIFT_VERSION = 5.0;
				TARGETED_DEVICE_FAMILY = "1,2";
			}};
			name = Release;
		}};
/* End XCBuildConfiguration section */

/* Begin XCConfigurationList section */
		C0DE0000000000000000E001 /* Build configuration list for PBXProject "{name}" */ = {{
			isa = XCConfigurationList;
			buildConfigurations = (
				C0DE0000000000000000C001 /* Debug */,
				C0DE0000000000000000C002 /* Release */,
			);
			defaultConfigurationIsVisible = 0;
			defaultConfigurationName = Release;
		}};
		C0DE0000000000000000E002 /* Build configuration list for PBXNativeTarget "{name}" */ = {{
			isa = XCConfigurationList;
			buildConfigurations = (
				C0DE0000000000000000C003 /* Debug */,
				C0DE0000000000000000C004 /* Release */,
			);
			defaultConfigurationIsVisible = 0;
			defaultConfigurationName = Release;
		}};
/* End XCConfigurationList section */
	}};
	rootObject = C0DE0000000000000000A001 /* Project object */;
}}
"#,
        name = name,
        lib_name = lib_name,
        version = product.version,
        identifier = product.identifier,
        xcframework_ref = xcframework_ref,
        target_id = TARGET_ID,
    )
}

/// 共享 scheme（`xcodebuild archive -scheme` 的必需输入）。
///
/// 裸 CI 上 Xcode 不会为工程自动生成 scheme，缺它 `xcodebuild` 直接报
/// `does not contain a scheme named …`——故骨架自带一份共享 scheme。
pub fn skeleton_scheme(product: &ProductInfo) -> String {
    let name = &product.name;
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<Scheme LastUpgradeVersion = "1500" version = "1.7">
   <BuildAction parallelizeBuildables = "YES" buildImplicitDependencies = "YES">
      <BuildActionEntries>
         <BuildActionEntry buildForTesting = "YES" buildForRunning = "YES" buildForProfiling = "YES" buildForArchiving = "YES" buildForAnalyzing = "YES">
            <BuildableReference
               BuildableIdentifier = "primary"
               BlueprintIdentifier = "{target_id}"
               BuildableName = "{name}.app"
               BlueprintName = "{name}"
               ReferencedContainer = "container:{name}.xcodeproj">
            </BuildableReference>
         </BuildActionEntry>
      </BuildActionEntries>
   </BuildAction>
   <TestAction buildConfiguration = "Debug" selectedDebuggerIdentifier = "Xcode.DebuggerFoundation.Debugger.LLDB" selectedLauncherIdentifier = "Xcode.DebuggerFoundation.Launcher.LLDB" shouldUseLaunchSchemeArgsEnv = "YES">
      <Testables>
      </Testables>
   </TestAction>
   <LaunchAction buildConfiguration = "Debug" selectedDebuggerIdentifier = "Xcode.DebuggerFoundation.Debugger.LLDB" selectedLauncherIdentifier = "Xcode.DebuggerFoundation.Launcher.LLDB" launchStyle = "0" useCustomWorkingDirectory = "NO" ignoresPersistentStateOnLaunch = "NO" debugDocumentVersioning = "YES" debugServiceExtension = "internal" allowLocationSimulation = "YES">
      <BuildableProductRunnable runnableDebuggingMode = "0">
         <BuildableReference
            BuildableIdentifier = "primary"
            BlueprintIdentifier = "{target_id}"
            BuildableName = "{name}.app"
            BlueprintName = "{name}"
            ReferencedContainer = "container:{name}.xcodeproj">
         </BuildableReference>
      </BuildableProductRunnable>
   </LaunchAction>
   <ProfileAction buildConfiguration = "Release" shouldUseLaunchSchemeArgsEnv = "YES" savedToolIdentifier = "" useCustomWorkingDirectory = "NO" debugDocumentVersioning = "YES">
   </ProfileAction>
   <AnalyzeAction buildConfiguration = "Debug"/>
   <ArchiveAction buildConfiguration = "Release" revealArchiveInOrganizer = "YES"/>
</Scheme>
"#,
        name = name,
        target_id = TARGET_ID,
    )
}

/// 骨架 `Info.plist`（iOS 键位最小集）。
pub fn skeleton_info_plist(product: &ProductInfo) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleDevelopmentRegion</key><string>en</string>
    <key>CFBundleDisplayName</key><string>{name}</string>
    <key>CFBundleExecutable</key><string>$(EXECUTABLE_NAME)</string>
    <key>CFBundleIdentifier</key><string>{identifier}</string>
    <key>CFBundleInfoDictionaryVersion</key><string>6.0</string>
    <key>CFBundleName</key><string>$(PRODUCT_NAME)</string>
    <key>CFBundlePackageType</key><string>APPL</string>
    <key>CFBundleShortVersionString</key><string>{version}</string>
    <key>CFBundleVersion</key><string>1</string>
    <key>LSRequiresIPhoneOS</key><true/>
    <key>UILaunchScreen</key><dict/>
    <key>UISupportedInterfaceOrientations</key>
    <array>
        <string>UIInterfaceOrientationPortrait</string>
    </array>
</dict>
</plist>
"#,
        name = product.name,
        identifier = product.identifier,
        version = product.version,
    )
}

/// 骨架入口（SwiftUI 最小 App；桥绑定文件由工程 Sources 编译校验 modulemap）。
pub fn skeleton_app_swift(product: &ProductInfo) -> String {
    format!(
        r#"import SwiftUI

// PolyPack 生成的宿主骨架（T-M2-08）：桥绑定模块经工程 Sources 编译，
// 真实宿主页面由 RN 宿主模板（T-M2-04）替换本文件。
@main
struct {ident}App: App {{
    var body: some Scene {{
        WindowGroup {{
            ContentView()
        }}
    }}
}}

struct ContentView: View {{
    var body: some View {{
        Text("{name}")
    }}
}}
"#,
        ident = swift_ident(product),
        name = product.name,
    )
}

/// 导出配置（`xcodebuild -exportArchive -exportOptionsPlist` 的输入）。
///
/// `signingStyle = manual` + 空 teamID：导出未签名 IPA；证书 / 描述文件由 T-M2-10
/// 签名链在构建地按需注入（不放此处，避免明文凭据入库）。
pub fn export_options_plist(method: &str) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>method</key><string>{method}</string>
    <key>signingStyle</key><string>manual</string>
    <key>stripSwiftSymbols</key><true/>
    <key>compileBitcode</key><false/>
</dict>
</plist>
"#
    )
}

/// Swift 标识符形态（App.swift 的 `struct <ident>App`；非字母数字丢弃，数字开头前置 `App`）。
fn swift_ident(product: &ProductInfo) -> String {
    let mut ident: String = product
        .name
        .chars()
        .filter(|ch| ch.is_alphanumeric())
        .collect();
    if ident.is_empty() {
        ident = "PolyPack".to_string();
    }
    if ident.chars().next().is_some_and(|ch| ch.is_ascii_digit()) {
        ident.insert_str(0, "App");
    }
    ident
}

// ---------------------------------------------------------------- 命令组装

/// archive → exportArchive → 校验（plutil / xcrun lipo）的完整命令序。
fn build_commands(project: &Path, inputs: &BundleInputs) -> Vec<Command> {
    let name = &inputs.product.name;
    let archive = inputs.out_dir.join(format!("{name}.xcarchive"));
    let export = export_dir(inputs);
    let app = export.join("Payload").join(format!("{name}.app"));
    vec![
        Command::new(
            "xcodebuild",
            [
                "archive".into(),
                "-project".into(),
                path_arg(project),
                "-scheme".into(),
                name.clone(),
                "-configuration".into(),
                "Release".into(),
                "-destination".into(),
                "generic/platform=iOS".into(),
                "-archivePath".into(),
                path_arg(&archive),
                // 未签名归档：签名位由 T-M2-10 接入
                "CODE_SIGNING_ALLOWED=NO".into(),
                "CODE_SIGNING_REQUIRED=NO".into(),
                "CODE_SIGN_IDENTITY=".into(),
            ],
        ),
        Command::new(
            "xcodebuild",
            [
                "-exportArchive".into(),
                "-archivePath".into(),
                path_arg(&archive),
                "-exportOptionsPlist".into(),
                path_arg(&export_options_path(inputs)),
                "-exportPath".into(),
                path_arg(&export),
            ],
        ),
        // 产物校验（T-M2-08 验收）：Info.plist 语法 + 架构
        Command::new(
            "plutil",
            ["-lint".into(), path_arg(&app.join("Info.plist"))],
        ),
        Command::new(
            "xcrun",
            ["lipo".into(), "-info".into(), path_arg(&app.join(name))],
        ),
    ]
}

/// 导出方式（plan / package 读同一函数，保证落盘与产物流一致）。
fn export_method() -> Result<String, BundleError> {
    parse_export_method(std::env::var(EXPORT_METHOD_ENV).ok().as_deref())
}

/// 导出方式解析（纯函数：`None` → 缺省 `development`；白名单外响亮拒绝）。
fn parse_export_method(raw: Option<&str>) -> Result<String, BundleError> {
    let method = raw.unwrap_or("development");
    if EXPORT_METHODS.contains(&method) {
        return Ok(method.to_string());
    }
    Err(BundleError::InvalidInput {
        packager: IOS,
        field: EXPORT_METHOD_ENV,
        value: method.to_string(),
        reason: format!("合法导出方式：{}", EXPORT_METHODS.join(" / ")),
    })
}

// ---------------------------------------------------------------- 定位与路径

/// 取出 iOS 输入（目标 id / 产品元数据 / `ios` 段校验；缺失即报错）。
pub(crate) fn ios_inputs<'a>(
    inputs: &'a BundleInputs,
    spec: &TargetSpec,
) -> Result<&'a IosInputs, BundleError> {
    if spec.id != IOS {
        return Err(BundleError::InvalidInput {
            packager: IOS,
            field: "spec.id",
            value: spec.id.clone(),
            reason: "iOS 打包器只服务 `ios` 目标".to_string(),
        });
    }
    inputs.product.validate(IOS)?;
    inputs.ios.as_ref().ok_or(BundleError::MissingInput {
        packager: IOS,
        field: "ios",
    })
}

/// 宿主门控：iOS 打包链仅 macOS 宿主可跑（xcodebuild / plutil / xcrun / lipo）。
pub(crate) fn require_macos(tool: &str) -> Result<(), BundleError> {
    if poly_targets::current_host() == Some(poly_targets::Host::Macos) {
        return Ok(());
    }
    Err(BundleError::ToolMissing {
        packager: IOS,
        tool: tool.to_string(),
        hint: "Apple 工具链仅随 Xcode 提供（xcodebuild / plutil / xcrun / lipo）：\
               iOS 链路需 macOS 宿主（Linux 发起走远程农场 macOS worker，T-M2-13）"
            .to_string(),
    })
}

/// 打包格式与打包器声明必须一致（防「按 ipa 目标产出别的格式」）。
fn check_format(spec: &TargetSpec, want: &str) -> Result<(), BundleError> {
    if spec.package.format == want {
        return Ok(());
    }
    Err(BundleError::InvalidInput {
        packager: IOS,
        field: "spec.package.format",
        value: spec.package.format.clone(),
        reason: format!("打包器要求格式 `{want}`（polypack.toml 的 [targets.*].package.format）"),
    })
}

/// 导出目录（`xcodebuild -exportArchive -exportPath`）。
fn export_dir(inputs: &BundleInputs) -> PathBuf {
    inputs.out_dir.join("export")
}

/// ExportOptions.plist 落点（骨架 / 复用路径共用）。
fn export_options_path(inputs: &BundleInputs) -> PathBuf {
    inputs.out_dir.join("ExportOptions.plist")
}

/// 导出目录内的 `.ipa`（exportArchive 产物；不递归——布局固定）。
fn find_ipa(export_dir: &Path) -> Option<PathBuf> {
    std::fs::read_dir(export_dir)
        .ok()?
        .flatten()
        .map(|entry| entry.path())
        .find(|path| path.extension().is_some_and(|ext| ext == "ipa"))
}

/// 参数形态（路径一律显式传参，避免拼接成 shell 串）。
fn path_arg(path: &Path) -> String {
    path.display().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn product() -> ProductInfo {
        ProductInfo {
            name: "Pax Notes".to_string(),
            version: "1.2.3".to_string(),
            identifier: "com.polypack.paxnotes".to_string(),
            manufacturer: "PolyPack".to_string(),
            description: "示例".to_string(),
            homepage: None,
        }
    }

    /// 共享 scheme 与 pbxproj 的 target id 必须一致（xcodebuild 按 BlueprintIdentifier 解析）。
    #[test]
    fn scheme_target_id_matches_pbxproj() {
        let product = product();
        let scheme = skeleton_scheme(&product);
        assert!(
            scheme.contains(&format!("BlueprintIdentifier = \"{TARGET_ID}\"")),
            "scheme 未引用 app target：{scheme}"
        );
        assert!(
            scheme.contains("container:Pax Notes.xcodeproj"),
            "scheme 的容器须指向骨架工程：{scheme}"
        );
        let pbxproj = skeleton_pbxproj(
            &product,
            "pax_core",
            Path::new("/out/Pax Notes.xcframework"),
        );
        assert!(
            pbxproj.contains(&format!("{TARGET_ID} /* Pax Notes */ = {{")),
            "pbxproj 的 target id 与 scheme 不一致"
        );
    }

    /// Swift 标识符派生：非字母数字丢弃、数字开头前置。
    #[test]
    fn swift_ident_is_a_valid_identifier() {
        assert_eq!(swift_ident(&product()), "PaxNotes");
        let mut numeric = product();
        numeric.name = "2048 Game".to_string();
        assert_eq!(swift_ident(&numeric), "App2048Game");
    }

    /// 导出方式白名单：缺省值 + 非法值响亮拒绝。
    #[test]
    fn export_method_whitelist() {
        assert_eq!(parse_export_method(None).unwrap(), "development");
        assert_eq!(
            parse_export_method(Some("app-store-connect")).unwrap(),
            "app-store-connect"
        );
        let err = parse_export_method(Some("bogus")).expect_err("非法导出方式应报错");
        assert!(matches!(err, BundleError::InvalidInput { .. }));
    }
}
