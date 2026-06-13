#import "espeak-ng/bundle.h"
#import "espeak-ng/espeak_ng.h"

const NSErrorDomain EspeakErrorDomain = @"EspeakErrorDomain";

@implementation EspeakLib
+ (BOOL)ensureBundleInstalledInRoot:(NSURL*_Nonnull)root error:(NSError*_Nullable*_Nonnull)error {
  NSFileManager *fm = [NSFileManager defaultManager];
  NSURL *dataRoot = [root URLByAppendingPathComponent:@"espeak-ng-data"];

  NSBundle *bundle = [NSBundle bundleWithPath:@"espeak-ng_data.bundle"];
  if (!bundle) {
    NSURL *url = [[NSBundle mainBundle] URLForResource:@"espeak-ng_data" withExtension:@"bundle"];
    if (url) {
      bundle = [NSBundle bundleWithURL:url];
    }
  }
  if (!bundle) {
    for (NSBundle *b in [NSBundle allBundles]) {
      NSString *lastComp = [b.bundlePath lastPathComponent];
      if ([lastComp isEqualToString:@"espeak-ng_data.bundle"] || [lastComp isEqualToString:@"espeak-ng-data.bundle"]) {
        bundle = b;
        break;
      }
      NSString *path = [b pathForResource:@"espeak-ng_data" ofType:@"bundle"];
      if (path) {
        bundle = [NSBundle bundleWithPath:path];
        break;
      }
      path = [b pathForResource:@"espeak-ng-data" ofType:nil];
      if (path) {
        bundle = b;
        break;
      }
    }
  }
  if (!bundle) {
    NSString *thisFile = [NSString stringWithUTF8String:__FILE__];
    NSString *dir = [thisFile stringByDeletingLastPathComponent]; // libespeak-ng
    dir = [dir stringByDeletingLastPathComponent]; // Sources
    dir = [dir stringByDeletingLastPathComponent]; // espeak-ng-spm
    dir = [dir stringByDeletingLastPathComponent]; // Vendor
    NSString *prosodiaSpeechRoot = [dir stringByDeletingLastPathComponent]; // ProsodiaActor
    NSString *prosodiaRoot = [prosodiaSpeechRoot stringByDeletingLastPathComponent]; // Prosodia
    
    NSArray *roots = @[prosodiaSpeechRoot, prosodiaRoot];
    for (NSString *rootPath in roots) {
      NSString *buildPath = [rootPath stringByAppendingPathComponent:@".build"];
      if ([fm fileExistsAtPath:buildPath]) {
        NSDirectoryEnumerator *enumerator = [fm enumeratorAtURL:[NSURL fileURLWithPath:buildPath]
                                     includingPropertiesForKeys:nil
                                                        options:NSDirectoryEnumerationSkipsHiddenFiles
                                                   errorHandler:nil];
        for (NSURL *url in enumerator) {
          if ([[url lastPathComponent] isEqualToString:@"espeak-ng_data.bundle"]) {
            bundle = [NSBundle bundleWithURL:url];
            if (bundle) break;
          }
        }
      }
      if (bundle) break;
    }
  }
  if (!bundle) {
    if (error) {
      *error = [NSError errorWithDomain:EspeakErrorDomain
                                   code:1
                               userInfo:@{ NSLocalizedDescriptionKey: @"Could not find espeak-ng_data.bundle in main bundle or allBundles." }];
    }
    return NO;
  }
  NSURL *bdl = [bundle resourceURL];
  if (![fm fileExistsAtPath:[bdl URLByAppendingPathComponent:@"espeak-ng-data"].path]) {
    bdl = bundle.bundleURL;
  }


  NSURL *bundleCheckURL = [[bdl URLByAppendingPathComponent:@"phsource"] URLByAppendingPathComponent:@"phonemes"];
  NSDate *bundleDate = [[fm attributesOfItemAtPath:bundleCheckURL.path error:nil] objectForKey:NSFileModificationDate];
  NSDate *installDate = [[fm attributesOfItemAtPath:dataRoot.path error:nil] objectForKey:NSFileModificationDate];

  if (installDate && bundleDate && [bundleDate compare:installDate] == NSOrderedDescending) {
    [fm removeItemAtURL:dataRoot error:nil];
    NSLog(@"UPDATE DATA: %@ -> %@", installDate, bundleDate);
  }

  FILE *nullout = nil;
  if (![fm fileExistsAtPath:dataRoot.path]) {
    nullout = fopen("/dev/null", "w");

    if (![fm copyItemAtURL:[bdl URLByAppendingPathComponent:@"espeak-ng-data"] toURL:dataRoot error:error]) return NO;
    espeak_ng_InitializePath([root.path cStringUsingEncoding:NSUTF8StringEncoding]);
    NSString *ph_root = [bdl URLByAppendingPathComponent:@"phsource" isDirectory:YES].path;
    NSURL *dictbdl_root = [bdl URLByAppendingPathComponent:@"dictsource" isDirectory:YES];
    NSURL *dict_temp;
    NSString *dict_root = dictbdl_root.path;
    if ([fm fileExistsAtPath:[dictbdl_root URLByAppendingPathComponent:@"extra"].path]) {
      dict_temp = [[fm temporaryDirectory] URLByAppendingPathComponent:@"dictsource" isDirectory:YES];
      [fm removeItemAtURL:dict_temp error:nil];
      if (![fm copyItemAtURL:dictbdl_root toURL:dict_temp error:error]) return NO;
      NSArray<NSURL*> *extra_dicts = [fm contentsOfDirectoryAtURL:[dictbdl_root URLByAppendingPathComponent:@"extra"] includingPropertiesForKeys:nil options:NSDirectoryEnumerationSkipsSubdirectoryDescendants error:error];
      if (!extra_dicts) return NO;
      for (NSURL *u in extra_dicts) {
        if (![fm copyItemAtURL:u toURL:[dict_temp URLByAppendingPathComponent:u.lastPathComponent] error:error]) return NO;
      }
      dict_root = dict_temp.path;
    }

    espeak_ng_STATUS res;
    char errorbuf[256];
    if ((res = espeak_ng_CompileIntonationPath([ph_root cStringUsingEncoding:NSUTF8StringEncoding], nil, nullout, nil)) != ENS_OK) {
      espeak_ng_GetStatusCodeMessage(res, errorbuf, sizeof(errorbuf));
      *error = [NSError errorWithDomain:EspeakErrorDomain code:res userInfo:@{ NSLocalizedDescriptionKey: [NSString stringWithUTF8String:errorbuf] }];
      goto fail;
    }
    if ((res = espeak_ng_CompilePhonemeDataPath(22050, [ph_root cStringUsingEncoding:NSUTF8StringEncoding], nil, nullout, nil)) != ENS_OK) {
      espeak_ng_GetStatusCodeMessage(res, errorbuf, sizeof(errorbuf));
      *error = [NSError errorWithDomain:EspeakErrorDomain code:res userInfo:@{ NSLocalizedDescriptionKey: [NSString stringWithUTF8String:errorbuf] }];
      goto fail;
    }

    NSArray<NSURL*>* dict_files = [fm contentsOfDirectoryAtURL:[NSURL fileURLWithPath:dict_root] includingPropertiesForKeys:nil options:NSDirectoryEnumerationSkipsSubdirectoryDescendants error:error];
    if (!dict_files) return NO;
    NSMutableSet<NSString*>* dict_names = [NSMutableSet new];
    espeak_VOICE v;
    for (NSURL *u in dict_files) {
      NSArray<NSString*>* comps = [[u lastPathComponent] componentsSeparatedByString:@"_"];
      if (comps.count != 2) continue;
      if (![comps.lastObject isEqualToString:@"rules"]) continue;
      NSString *d = comps.firstObject;

      bzero(&v, sizeof(v));
      v.languages = [d cStringUsingEncoding:NSUTF8StringEncoding];
      if ((res = espeak_ng_SetVoiceByProperties(&v)) != ENS_OK) {
        espeak_ng_GetStatusCodeMessage(res, errorbuf, sizeof(errorbuf));
        *error = [NSError errorWithDomain:EspeakErrorDomain code:res userInfo:@{ NSLocalizedDescriptionKey: [NSString stringWithUTF8String:errorbuf] }];
        goto fail;
      }
      if ((res = espeak_ng_CompileDictionary([[dict_root stringByAppendingString:@"/"] cStringUsingEncoding:NSUTF8StringEncoding], [d cStringUsingEncoding:NSUTF8StringEncoding], nullout, 0, nil)) != ENS_OK) {
        espeak_ng_GetStatusCodeMessage(res, errorbuf, sizeof(errorbuf));
        *error = [NSError errorWithDomain:EspeakErrorDomain code:res userInfo:@{ NSLocalizedDescriptionKey: [NSString stringWithUTF8String:errorbuf] }];
        goto fail;
      }
    }
    fclose(nullout);
    if (dict_temp) [fm removeItemAtURL:dict_temp error:nil];
  }
  return YES;
fail:
  fclose(nullout);
  [fm removeItemAtURL:dataRoot error:nil];
  return NO;
}
@end
